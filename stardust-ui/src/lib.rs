//! Synthetic StarDust Dir/File tree. HTTP tree is built from BoundClass methods.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path as FsPath, PathBuf};

use serde::Serialize;
use stardust::{
    BoundClass, Bump, CallArg, CallRet, CallScalar, Class, Endianness, Error, Import, Ir, Lang,
    Library, MethodLoader, Param, Path, Region, Ret, Scalar, StructBuilder, Type, Value,
};

pub mod app_logo;
pub mod rebuild;
pub mod shell;

pub const KIND_FILE: u64 = 0;
pub const KIND_DIR: u64 = 1;
pub const NAME_CAP: u64 = 64;
pub const MAX_CHILD: usize = 64;
const REGION_SIZE: u64 = 4 * 1024 * 1024;
const SCRATCH_TAIL: u64 = 128;
const CONTENT_MAX: usize = 512 * 1024;

pub fn lib_path() -> &'static str {
    env!("STARDUST_FS_LIB")
}

pub fn node_layout() -> Type {
    StructBuilder::new()
        .field("name_len", Type::scalar(Scalar::U64))
        .field("name", Type::array(Type::scalar(Scalar::U8), 64).unwrap())
        .field("kind", Type::scalar(Scalar::U8))
        .repr_c()
        .unwrap()
}

pub fn file_layout() -> Type {
    StructBuilder::new()
        .field("name_len", Type::scalar(Scalar::U64))
        .field("name", Type::array(Type::scalar(Scalar::U8), 64).unwrap())
        .field("kind", Type::scalar(Scalar::U8))
        .field("size", Type::scalar(Scalar::U64))
        .repr_c()
        .unwrap()
}

pub fn dir_layout() -> Type {
    StructBuilder::new()
        .field("name_len", Type::scalar(Scalar::U64))
        .field("name", Type::array(Type::scalar(Scalar::U8), 64).unwrap())
        .field("kind", Type::scalar(Scalar::U8))
        .field("nchild", Type::scalar(Scalar::U64))
        .field("child", Type::array(Type::scalar(Scalar::U64), 64).unwrap())
        .repr_c()
        .unwrap()
}

fn ptr_len_u64(symbol: &str) -> Import {
    Import::new(
        symbol,
        vec![Param::Ptr, Param::Len],
        Ret::Scalar(Scalar::U64),
    )
    .with_lang(Lang::C)
}

pub fn fs_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("NodeLay", node_layout());
    ir.insert("FileLay", file_layout());
    ir.insert("DirLay", dir_layout());

    let mut node = Class {
        layout: "NodeLay".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    node.insert_method("kind", ptr_len_u64("fs_kind")).unwrap();
    node.insert_method(
        "name",
        Import::new(
            "fs_name",
            vec![
                Param::Ptr,
                Param::Len,
                Param::Ptr,
                Param::Scalar(Scalar::U64),
            ],
            Ret::Scalar(Scalar::U64),
        )
        .with_lang(Lang::C),
    )
    .unwrap();
    ir.insert_class("Node", node).unwrap();

    let file = Class {
        layout: "FileLay".into(),
        base: Some("Node".into()),
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    ir.insert_class("File", file).unwrap();

    let mut dir = Class {
        layout: "DirLay".into(),
        base: Some("Node".into()),
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    dir.insert_method("child_count", ptr_len_u64("fs_child_count"))
        .unwrap();
    dir.insert_method(
        "child_at",
        Import::new(
            "fs_child_at",
            vec![Param::Ptr, Param::Len, Param::Scalar(Scalar::U64)],
            Ret::Scalar(Scalar::U64),
        )
        .with_lang(Lang::C),
    )
    .unwrap();
    ir.insert_class("Dir", dir).unwrap();
    ir
}

fn store_name(ir: &Ir, layout: &str, region: &mut Region, offset: u64, name: &str) {
    let bytes = name.as_bytes();
    let take = bytes.len().min(NAME_CAP as usize);
    ir.store_at(
        layout,
        region,
        offset,
        &Path::new().name("name_len"),
        &Value::U64(take as u64),
    )
    .unwrap();
    let mut arr = vec![Value::U8(0); NAME_CAP as usize];
    for (i, b) in bytes.iter().take(take).enumerate() {
        arr[i] = Value::U8(*b);
    }
    ir.store_at(
        layout,
        region,
        offset,
        &Path::new().name("name"),
        &Value::Array(arr),
    )
    .unwrap();
}

fn store_kind(ir: &Ir, layout: &str, region: &mut Region, offset: u64, kind: u8) {
    ir.store_at(
        layout,
        region,
        offset,
        &Path::new().name("kind"),
        &Value::U8(kind),
    )
    .unwrap();
}

fn skip_dir_name(name: &str) -> bool {
    matches!(name, "target" | "vendor" | ".git")
}

fn entry_name(path: &FsPath) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".into())
}

enum OsSpec {
    File { name: String, size: u64 },
    Dir { name: String, children: Vec<OsSpec> },
}

struct Placed {
    offset: u64,
    name: String,
    kind: u8,
    size: u64,
    children: Vec<Placed>,
}

fn scan_os(path: &FsPath) -> OsSpec {
    if path.is_dir() {
        let mut children = Vec::new();
        if let Ok(rd) = fs::read_dir(path) {
            let mut ents: Vec<_> = rd.filter_map(|e| e.ok()).collect();
            ents.sort_by_key(|e| e.file_name());
            for e in ents {
                let n = e.file_name().to_string_lossy().into_owned();
                if skip_dir_name(&n) {
                    continue;
                }
                children.push(scan_os(&e.path()));
                if children.len() >= MAX_CHILD {
                    break;
                }
            }
        }
        OsSpec::Dir {
            name: entry_name(path),
            children,
        }
    } else {
        OsSpec::File {
            name: entry_name(path),
            size: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        }
    }
}

fn place_spec(
    spec: &OsSpec,
    bump: &mut Bump<'_>,
    dir: &BoundClass<'static>,
    file: &BoundClass<'static>,
) -> Result<Placed, Error> {
    match spec {
        OsSpec::File { name, size } => Ok(Placed {
            offset: file.place(bump)?,
            name: name.clone(),
            kind: KIND_FILE as u8,
            size: *size,
            children: Vec::new(),
        }),
        OsSpec::Dir { name, children } => {
            let offset = dir.place(bump)?;
            let mut placed_kids = Vec::new();
            for c in children {
                placed_kids.push(place_spec(c, bump, dir, file)?);
            }
            Ok(Placed {
                offset,
                name: name.clone(),
                kind: KIND_DIR as u8,
                size: 0,
                children: placed_kids,
            })
        }
    }
}

fn write_spec(ir: &Ir, region: &mut Region, p: &Placed) -> Result<(), Error> {
    let layout = if p.kind == KIND_DIR as u8 {
        "DirLay"
    } else {
        "FileLay"
    };
    store_name(ir, layout, region, p.offset, &p.name);
    store_kind(ir, layout, region, p.offset, p.kind);
    if p.kind == KIND_FILE as u8 {
        ir.store_at(
            layout,
            region,
            p.offset,
            &Path::new().name("size"),
            &Value::U64(p.size),
        )?;
    } else {
        ir.store_at(
            layout,
            region,
            p.offset,
            &Path::new().name("nchild"),
            &Value::U64(p.children.len() as u64),
        )?;
        for (i, ch) in p.children.iter().enumerate() {
            ir.store_at(
                layout,
                region,
                p.offset,
                &Path::new().name("child").index(i as u64),
                &Value::U64(ch.offset),
            )?;
            write_spec(ir, region, ch)?;
        }
    }
    Ok(())
}

pub struct World {
    pub ir: Ir,
    pub region: Region,
    pub root: u64,
    pub scratch: u64,
    os_root: PathBuf,
    node: BoundClass<'static>,
    dir: BoundClass<'static>,
}

impl World {
    pub fn demo_root() -> PathBuf {
        crate::shell::git_root()
    }

    pub fn seed() -> Result<Self, Error> {
        Self::seed_from(&Self::demo_root())
    }

    pub fn seed_from(root: &FsPath) -> Result<Self, Error> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let ir = fs_ir();
        let lib = Box::leak(Box::new(Library::load(lib_path())?));
        let mut loaders_node = BTreeMap::new();
        loaders_node.insert("kind".into(), MethodLoader::Library(lib));
        loaders_node.insert("name".into(), MethodLoader::Library(lib));
        let mut loaders_dir = BTreeMap::new();
        loaders_dir.insert("kind".into(), MethodLoader::Library(lib));
        loaders_dir.insert("name".into(), MethodLoader::Library(lib));
        loaders_dir.insert("child_count".into(), MethodLoader::Library(lib));
        loaders_dir.insert("child_at".into(), MethodLoader::Library(lib));
        let node = BoundClass::bind(&ir, "Node", &loaders_node)?;
        let dir = BoundClass::bind(&ir, "Dir", &loaders_dir)?;
        let file = BoundClass::bind(&ir, "File", &loaders_node)?;

        let spec = scan_os(&root);
        let mut region = Region::allocate("fs-viz", REGION_SIZE as usize, 8)?;
        let placed = {
            let mut bump = Bump::new(&mut region);
            place_spec(&spec, &mut bump, &dir, &file)?
        };
        write_spec(&ir, &mut region, &placed)?;
        let scratch = REGION_SIZE - SCRATCH_TAIL;

        Ok(Self {
            ir,
            region,
            root: placed.offset,
            scratch,
            os_root: root,
            node,
            dir,
        })
    }

    fn call_u64(
        bound: &BoundClass<'static>,
        method: &str,
        region: &mut Region,
        offset: u64,
        extra: &[CallArg],
    ) -> Result<u64, Error> {
        match bound.call_with(method, region, offset, extra)? {
            CallRet::Scalar(CallScalar::U64(v)) => Ok(v),
            other => panic!("expected U64 ret, got {other:?}"),
        }
    }

    fn read_name(&mut self, offset: u64) -> Result<String, Error> {
        let scratch = self.scratch;
        let len = Self::call_u64(
            &self.node,
            "name",
            &mut self.region,
            offset,
            &[
                CallArg::Ptr {
                    region: 0,
                    offset: scratch,
                },
                CallArg::Scalar(CallScalar::U64(NAME_CAP)),
            ],
        )?;
        let start = scratch as usize;
        let end = start + len as usize;
        let bytes = &self.region.as_bytes()[start..end];
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn read_kind(&mut self, offset: u64) -> Result<u64, Error> {
        Self::call_u64(&self.node, "kind", &mut self.region, offset, &[])
    }

    pub fn tree(&mut self) -> Result<TreeNode, Error> {
        self.walk(self.root, None)
    }

    fn walk(&mut self, offset: u64, parent_rel: Option<&str>) -> Result<TreeNode, Error> {
        let name = self.read_name(offset)?;
        let rel = match parent_rel {
            None => String::new(),
            Some("") => name.clone(),
            Some(p) => format!("{p}/{name}"),
        };
        let kind = self.read_kind(offset)?;
        if kind == KIND_DIR {
            let n = Self::call_u64(&self.dir, "child_count", &mut self.region, offset, &[])?;
            let mut children = Vec::new();
            for i in 0..n {
                let child = Self::call_u64(
                    &self.dir,
                    "child_at",
                    &mut self.region,
                    offset,
                    &[CallArg::Scalar(CallScalar::U64(i))],
                )?;
                children.push(self.walk(child, Some(rel.as_str()))?);
            }
            Ok(TreeNode {
                offset,
                name,
                path: rel,
                kind: "dir".into(),
                children: Some(children),
            })
        } else {
            Ok(TreeNode {
                offset,
                name,
                path: rel,
                kind: "file".into(),
                children: None,
            })
        }
    }

    pub fn snapshot(&mut self) -> Result<Snapshot, Error> {
        let tree = self.tree()?;
        let mut details = BTreeMap::new();
        collect_details(self, &tree, &mut details)?;
        Ok(Snapshot { tree, details })
    }

    pub fn detail(&self, node: &TreeNode) -> Result<NodeDetail, Error> {
        let kind = match self.ir.retrieve_at(
            "NodeLay",
            &self.region,
            node.offset,
            &Path::new().name("kind"),
        )? {
            Value::U8(k) => k,
            _ => return Err(Error::TypeMismatch),
        };
        let (layout, class, kind_s) = if kind as u64 == KIND_DIR {
            ("DirLay", "Dir", "dir")
        } else {
            ("FileLay", "File", "file")
        };
        let fields = self
            .ir
            .retrieve_at(layout, &self.region, node.offset, &Path::new())?;
        let path = if node.path.is_empty() {
            node.name.clone()
        } else {
            node.path.clone()
        };
        let (content, binary) = if kind_s == "file" {
            read_file_body(&self.os_root, &node.path)
        } else {
            (None, false)
        };
        Ok(NodeDetail {
            class: class.into(),
            offset: node.offset,
            path,
            kind: kind_s.into(),
            fields: value_json(&fields),
            content,
            binary,
        })
    }
}

fn collect_details(
    world: &World,
    node: &TreeNode,
    out: &mut BTreeMap<u64, NodeDetail>,
) -> Result<(), Error> {
    out.insert(node.offset, world.detail(node)?);
    if let Some(children) = &node.children {
        for ch in children {
            collect_details(world, ch, out)?;
        }
    }
    Ok(())
}

#[derive(Clone)]
pub struct Snapshot {
    pub tree: TreeNode,
    pub details: BTreeMap<u64, NodeDetail>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TreeNode {
    pub offset: u64,
    pub name: String,
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<TreeNode>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NodeDetail {
    pub class: String,
    pub offset: u64,
    pub path: String,
    pub kind: String,
    pub fields: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub binary: bool,
}

fn value_json(v: &Value) -> serde_json::Value {
    match v {
        Value::I8(x) => serde_json::json!(x),
        Value::U8(x) => serde_json::json!(x),
        Value::I16(x) => serde_json::json!(x),
        Value::U16(x) => serde_json::json!(x),
        Value::I32(x) => serde_json::json!(x),
        Value::U32(x) => serde_json::json!(x),
        Value::I64(x) => serde_json::json!(x),
        Value::U64(x) => serde_json::json!(x),
        Value::F32(x) => serde_json::json!(x),
        Value::F64(x) => serde_json::json!(x),
        Value::Struct(fields) => {
            let mut map = serde_json::Map::new();
            for (k, val) in fields {
                map.insert(k.clone(), value_json(val));
            }
            serde_json::Value::Object(map)
        }
        Value::Array(elems) => {
            if let Some(s) = u8_array_as_string(elems) {
                serde_json::json!(s)
            } else {
                serde_json::Value::Array(elems.iter().map(value_json).collect())
            }
        }
        Value::Slice { offset, elems } => serde_json::json!({
            "offset": offset,
            "elems": elems.iter().map(value_json).collect::<Vec<_>>(),
        }),
    }
}

fn u8_array_as_string(elems: &[Value]) -> Option<String> {
    if elems.is_empty() || !elems.iter().all(|e| matches!(e, Value::U8(_))) {
        return None;
    }
    let bytes: Vec<u8> = elems
        .iter()
        .map(|e| match e {
            Value::U8(b) => *b,
            _ => 0,
        })
        .take_while(|b| *b != 0)
        .collect();
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_file_body(root: &FsPath, rel: &str) -> (Option<String>, bool) {
    if rel.is_empty() {
        return (None, false);
    }
    let path = root.join(rel);
    let Ok(mut bytes) = fs::read(&path) else {
        return (Some(format!("(could not read {rel})")), false);
    };
    if bytes.contains(&0) {
        return (None, true);
    }
    let truncated = bytes.len() > CONTENT_MAX;
    if truncated {
        bytes.truncate(CONTENT_MAX);
    }
    match String::from_utf8(bytes) {
        Ok(mut s) => {
            if truncated {
                s.push_str("\n\n… truncated");
            }
            (Some(s), false)
        }
        Err(_) => (None, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_via_methods() {
        let mut world = World::seed().expect("seed");
        let tree = world.tree().expect("tree");
        assert_eq!(tree.name, "StarDust");
        assert_eq!(tree.kind, "dir");
        let kids = tree.children.expect("root children");
        assert!(
            kids.iter().any(|k| k.name == "stardust" && k.kind == "dir"),
            "expected stardust/ in git root, got {:?}",
            kids.iter().map(|k| &k.name).collect::<Vec<_>>()
        );
        assert!(
            kids.iter()
                .any(|k| k.name == "stardust-ui" && k.kind == "dir"),
            "expected stardust-ui/ in git root"
        );
        assert!(
            kids.iter().all(|k| k.name != "target"),
            "target/ should be skipped"
        );
    }

    #[test]
    fn file_content_waves_md() {
        let mut world = World::seed().expect("seed");
        let tree = world.tree().expect("tree");
        let stardust = tree
            .children
            .as_ref()
            .expect("root children")
            .iter()
            .find(|k| k.name == "stardust" && k.kind == "dir")
            .expect("stardust/");
        let waves = stardust
            .children
            .as_ref()
            .expect("stardust children")
            .iter()
            .find(|k| k.name == "waves.md")
            .expect("stardust/waves.md");
        let d = world.detail(waves).expect("detail");
        assert_eq!(d.kind, "file");
        assert_eq!(d.class, "File");
        assert!(!d.binary);
        let body = d.content.expect("utf-8 body");
        assert!(
            body.contains("live contract"),
            "expected waves.md source, got {} chars",
            body.len()
        );
        assert!(
            d.fields.get("size").is_some(),
            "object view should include File.size"
        );
    }

    #[test]
    fn detail_from_views() {
        let mut world = World::seed().expect("seed");
        let tree = world.tree().expect("tree");
        let d = world.detail(&tree).expect("root detail");
        assert_eq!(d.class, "Dir");
        assert_eq!(d.kind, "dir");
        assert!(d.fields.get("nchild").is_some());
        assert!(d.content.is_none());
    }
}
