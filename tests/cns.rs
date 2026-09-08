//! CNS graph + Luna Light hosted on one WorldLay region (Python BoundClass).
//! Feature `python`. Named gates: `cns_smoke`, `luna_check_store`, `luna_smoke`,
//! `luna_dict_yaml`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use stardust::{
    BoundClass, Class, Endianness, Import, Ir, Lang, MethodLoader, Param, Path, Region, Ret,
    Scalar, StructBuilder, Type, Value,
};

const WORD_CAP: u64 = 8192;
const COMP_CAP: u64 = 1024;
const STROKE_TYPE_CAP: u64 = 8;
const WORD_IDS_CAP: u64 = 256;
const WORD_COMP_CAP: u64 = 32768;
const WORD_STROKE_CAP: u64 = 32768;
const ENTITY_CAP: u64 = 16;
const OVERRIDE_CAP: u64 = 256;
const BLOB_CAP: u64 = 4 * 1024 * 1024;

fn slice_u8() -> Type {
    Type::slice(Type::scalar(Scalar::U8))
}

fn py_void(symbol: &str) -> Import {
    Import::new(symbol, vec![Param::Ptr, Param::Len], Ret::Void).with_lang(Lang::Python)
}

fn cns_word_lay() -> Type {
    StructBuilder::new()
        .field("char", slice_u8())
        .field("cns", slice_u8())
        .field("strokes", slice_u8())
        .field("ids", slice_u8())
        .field("seq_level", Type::scalar(Scalar::U8))
        .field("has_seq_level", Type::scalar(Scalar::U8))
        .field("split_index", Type::scalar(Scalar::U8))
        .repr_c()
        .unwrap()
}

fn luna_word_lay() -> Type {
    StructBuilder::new()
        .field("char", slice_u8())
        .field("cns", slice_u8())
        .field("strokes", slice_u8())
        .field("ids", slice_u8())
        .field("seq_level", Type::scalar(Scalar::U8))
        .field("has_seq_level", Type::scalar(Scalar::U8))
        .field("split_index", Type::scalar(Scalar::U8))
        .field("encoding", slice_u8())
        .field("post_encoding", slice_u8())
        .repr_c()
        .unwrap()
}

fn cns_component_lay() -> Type {
    StructBuilder::new()
        .field("char", slice_u8())
        .field("cns", slice_u8())
        .field("unicode", slice_u8())
        .field("encoding", slice_u8())
        .repr_c()
        .unwrap()
}

fn stroke_type_lay() -> Type {
    StructBuilder::new()
        .field("code", Type::scalar(Scalar::U64))
        .field("name", slice_u8())
        .field("glyph", slice_u8())
        .repr_c()
        .unwrap()
}

fn word_ids_lay() -> Type {
    StructBuilder::new()
        .field("from", slice_u8())
        .field("seq", Type::scalar(Scalar::U8))
        .field("leaf_index", Type::scalar(Scalar::U8))
        .repr_c()
        .unwrap()
}

fn word_comp_lay() -> Type {
    StructBuilder::new()
        .field("from", slice_u8())
        .field("to", slice_u8())
        .field("seq", Type::scalar(Scalar::U8))
        .repr_c()
        .unwrap()
}

fn word_stroke_lay() -> Type {
    StructBuilder::new()
        .field("from", slice_u8())
        .field("to", slice_u8())
        .field("seq", Type::scalar(Scalar::U8))
        .repr_c()
        .unwrap()
}

fn ids_entity_lay() -> Type {
    StructBuilder::new()
        .field("entity", slice_u8())
        .field("component", slice_u8())
        .repr_c()
        .unwrap()
}

fn luna_override_lay() -> Type {
    StructBuilder::new()
        .field("char", slice_u8())
        .field("encoding", slice_u8())
        .field("post", slice_u8())
        .repr_c()
        .unwrap()
}

fn world_lay() -> Type {
    StructBuilder::new()
        .field("n_words", Type::scalar(Scalar::U64))
        .field("n_comps", Type::scalar(Scalar::U64))
        .field("n_stroke_types", Type::scalar(Scalar::U64))
        .field("n_word_ids", Type::scalar(Scalar::U64))
        .field("n_word_comp", Type::scalar(Scalar::U64))
        .field("n_word_stroke", Type::scalar(Scalar::U64))
        .field("n_ids_entity", Type::scalar(Scalar::U64))
        .field("n_override", Type::scalar(Scalar::U64))
        .field("blob_used", Type::scalar(Scalar::U64))
        .field("query", slice_u8())
        .field("result", slice_u8())
        .field("smoke_chk", Type::scalar(Scalar::U64))
        .field("mismatch_count", Type::scalar(Scalar::U64))
        .field("luna_smoke_chk", Type::scalar(Scalar::U64))
        .field("words", Type::array(luna_word_lay(), WORD_CAP).unwrap())
        .field(
            "components",
            Type::array(cns_component_lay(), COMP_CAP).unwrap(),
        )
        .field(
            "stroke_types",
            Type::array(stroke_type_lay(), STROKE_TYPE_CAP).unwrap(),
        )
        .field(
            "word_ids",
            Type::array(word_ids_lay(), WORD_IDS_CAP).unwrap(),
        )
        .field(
            "word_comp",
            Type::array(word_comp_lay(), WORD_COMP_CAP).unwrap(),
        )
        .field(
            "word_stroke",
            Type::array(word_stroke_lay(), WORD_STROKE_CAP).unwrap(),
        )
        .field(
            "ids_entity",
            Type::array(ids_entity_lay(), ENTITY_CAP).unwrap(),
        )
        .field(
            "overrides",
            Type::array(luna_override_lay(), OVERRIDE_CAP).unwrap(),
        )
        .field(
            "blob",
            Type::array(Type::scalar(Scalar::U8), BLOB_CAP).unwrap(),
        )
        .repr_c()
        .unwrap()
}

fn world_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("CnsWordLay", cns_word_lay());
    ir.insert("LunaWordLay", luna_word_lay());
    ir.insert("CnsComponentLay", cns_component_lay());
    ir.insert("StrokeTypeLay", stroke_type_lay());
    ir.insert("WordIdsLay", word_ids_lay());
    ir.insert("WordCompLay", word_comp_lay());
    ir.insert("WordStrokeLay", word_stroke_lay());
    ir.insert("IdsEntityLay", ids_entity_lay());
    ir.insert("LunaOverrideLay", luna_override_lay());
    ir.insert("WorldLay", world_lay());

    let cns_word = Class {
        layout: "CnsWordLay".into(),
        base: None,
        methods: Default::default(),
    };
    ir.insert_class("CnsWord", cns_word).unwrap();

    let luna_word = Class {
        layout: "LunaWordLay".into(),
        base: Some("CnsWord".into()),
        methods: Default::default(),
    };
    ir.insert_class("LunaWord", luna_word).unwrap();

    let mut graph = Class {
        layout: "WorldLay".into(),
        base: None,
        methods: Default::default(),
    };
    graph
        .insert_method("import_all", py_void("import_all"))
        .unwrap();
    graph
        .insert_method("components_of", py_void("components_of"))
        .unwrap();
    graph
        .insert_method("stroke_order", py_void("stroke_order"))
        .unwrap();
    graph.insert_method("smoke", py_void("smoke")).unwrap();
    ir.insert_class("CnsGraph", graph).unwrap();

    let mut store = Class {
        layout: "WorldLay".into(),
        base: Some("CnsGraph".into()),
        methods: Default::default(),
    };
    store
        .insert_method("persist_encodings", py_void("persist_encodings"))
        .unwrap();
    store
        .insert_method("check_store", py_void("check_store"))
        .unwrap();
    store
        .insert_method("check_dict", py_void("check_dict"))
        .unwrap();
    store
        .insert_method("smoke_luna", py_void("smoke_luna"))
        .unwrap();
    ir.insert_class("LunaStore", store).unwrap();
    ir
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cns_py() -> PathBuf {
    root().join("examples/cns_graph/cns.py")
}

fn luna_py() -> PathBuf {
    root().join("examples/cns_graph/luna.py")
}

fn world_sd() -> PathBuf {
    root().join("examples/cns_graph/world.sd")
}

fn bind_graph<'a>(ir: &Ir) -> BoundClass<'a> {
    let script = cns_py();
    let mut loaders = BTreeMap::new();
    for name in ["import_all", "components_of", "stroke_order", "smoke"] {
        loaders.insert(
            name.to_string(),
            MethodLoader::Python {
                script: script.clone(),
            },
        );
    }
    BoundClass::bind(ir, "CnsGraph", &loaders).expect("bind CnsGraph")
}

fn bind_store<'a>(ir: &Ir) -> BoundClass<'a> {
    let cns = cns_py();
    let luna = luna_py();
    let mut loaders = BTreeMap::new();
    for name in ["import_all", "components_of", "stroke_order", "smoke"] {
        loaders.insert(
            name.to_string(),
            MethodLoader::Python {
                script: cns.clone(),
            },
        );
    }
    for name in ["persist_encodings", "check_store", "check_dict", "smoke_luna"] {
        loaders.insert(
            name.to_string(),
            MethodLoader::Python {
                script: luna.clone(),
            },
        );
    }
    BoundClass::bind(ir, "LunaStore", &loaders).expect("bind LunaStore")
}

fn alloc_world(ir: &Ir) -> Region {
    let lay = ir.layout("WorldLay").unwrap();
    Region::allocate(
        "cns_world",
        lay.inline_size() as usize,
        lay.align() as usize,
    )
    .expect("allocate WorldLay")
}

fn u64_at(ir: &Ir, region: &Region, field: &str) -> u64 {
    match ir
        .retrieve("WorldLay", region, &Path::new().name(field))
        .unwrap()
    {
        Value::U64(v) => v,
        other => panic!("{field} not u64: {other:?}"),
    }
}

fn dump_layout_comments(ir: &Ir) -> String {
    let mut out = String::from("# Generated by tests/cns.rs STARDUST_DUMP_WORLD_SD=1. Do not hand-edit.\n");
    let Type::Struct(world) = ir.layout("WorldLay").unwrap() else {
        panic!("WorldLay");
    };
    out.push_str(&format!("WORLD_SIZE = {}\nWORLD_ALIGN = {}\n", world.size, world.align));
    for f in &world.fields {
        out.push_str(&format!(
            "OFF_{} = {}\n",
            f.name.to_ascii_uppercase(),
            f.offset
        ));
        if let Type::Array(a) = &f.ty {
            out.push_str(&format!(
                "STRIDE_{} = {}\nCAP_{} = {}\n",
                f.name.to_ascii_uppercase(),
                a.elem.inline_size(),
                f.name.to_ascii_uppercase(),
                a.count
            ));
        }
    }
    let Type::Struct(w) = ir.layout("LunaWordLay").unwrap() else {
        panic!("LunaWord");
    };
    for f in &w.fields {
        out.push_str(&format!(
            "WORD_{} = {}\n",
            f.name.to_ascii_uppercase(),
            f.offset
        ));
    }
    out.push_str(&format!("WORD_SIZE = {}\n", w.size));
    let Type::Struct(c) = ir.layout("CnsComponentLay").unwrap() else {
        panic!("comp");
    };
    for f in &c.fields {
        out.push_str(&format!(
            "COMP_{} = {}\n",
            f.name.to_ascii_uppercase(),
            f.offset
        ));
    }
    out.push_str(&format!("COMP_SIZE = {}\n", c.size));
    let Type::Struct(e) = ir.layout("WordCompLay").unwrap() else {
        panic!("wc");
    };
    for f in &e.fields {
        out.push_str(&format!(
            "EDGE_{} = {}\n",
            f.name.to_ascii_uppercase(),
            f.offset
        ));
    }
    out.push_str(&format!("EDGE_SIZE = {}\n", e.size));
    let Type::Struct(i) = ir.layout("WordIdsLay").unwrap() else {
        panic!("wi");
    };
    for f in &i.fields {
        out.push_str(&format!(
            "WIDS_{} = {}\n",
            f.name.to_ascii_uppercase(),
            f.offset
        ));
    }
    out.push_str(&format!("WIDS_SIZE = {}\n", i.size));
    let Type::Struct(s) = ir.layout("WordStrokeLay").unwrap() else {
        panic!("ws");
    };
    for f in &s.fields {
        out.push_str(&format!(
            "WSTR_{} = {}\n",
            f.name.to_ascii_uppercase(),
            f.offset
        ));
    }
    out.push_str(&format!("WSTR_SIZE = {}\n", s.size));
    let Type::Struct(en) = ir.layout("IdsEntityLay").unwrap() else {
        panic!("ent");
    };
    for f in &en.fields {
        out.push_str(&format!(
            "ENT_{} = {}\n",
            f.name.to_ascii_uppercase(),
            f.offset
        ));
    }
    let Type::Struct(ov) = ir.layout("LunaOverrideLay").unwrap() else {
        panic!("ov");
    };
    for f in &ov.fields {
        out.push_str(&format!(
            "OVR_{} = {}\n",
            f.name.to_ascii_uppercase(),
            f.offset
        ));
    }
    out
}

#[test]
fn dump_world_sd_if_env() {
    if std::env::var("STARDUST_DUMP_WORLD_SD").as_deref() != Ok("1") {
        return;
    }
    let ir = world_ir();
    std::fs::write(world_sd(), ir.to_sd().unwrap()).unwrap();
    std::fs::write(
        root().join("examples/cns_graph/layout_off.py"),
        dump_layout_comments(&ir),
    )
    .unwrap();
}

#[test]
fn world_sd_matches_builder() {
    let ir = world_ir();
    let on_disk = Ir::from_sd_path(world_sd()).expect("examples/cns_graph/world.sd");
    assert_eq!(ir, on_disk, "dumpable IR is authority; world.sd is the copy");
}

#[test]
fn luna_word_prefix() {
    let ir = world_ir();
    let _ = ir.class("LunaWord").unwrap();
}

#[test]
fn cns_smoke() {
    let ir = world_ir();
    let mut region = alloc_world(&ir);
    let bound = bind_graph(&ir);
    bound.call("import_all", &mut region, 0).unwrap();
    assert_eq!(u64_at(&ir, &region, "n_words"), 5428);
    assert_eq!(u64_at(&ir, &region, "n_comps"), 528);
    assert_eq!(u64_at(&ir, &region, "n_word_ids"), 152);
    assert_eq!(u64_at(&ir, &region, "n_word_comp"), 16989);
    assert_eq!(u64_at(&ir, &region, "n_word_stroke"), 16648);
    bound.call("smoke", &mut region, 0).unwrap();
    assert_eq!(u64_at(&ir, &region, "smoke_chk"), 39745);
}

#[test]
fn luna_check_store() {
    let ir = world_ir();
    let mut region = alloc_world(&ir);
    let bound = bind_store(&ir);
    bound.call("import_all", &mut region, 0).unwrap();
    bound.call("persist_encodings", &mut region, 0).unwrap();
    bound.call("check_store", &mut region, 0).unwrap();
    assert_eq!(u64_at(&ir, &region, "mismatch_count"), 0);
}

#[test]
fn luna_smoke() {
    let ir = world_ir();
    let mut region = alloc_world(&ir);
    let bound = bind_store(&ir);
    bound.call("import_all", &mut region, 0).unwrap();
    bound.call("smoke_luna", &mut region, 0).unwrap();
    let chk = u64_at(&ir, &region, "luna_smoke_chk");
    assert!(
        chk > 0,
        "luna_smoke wrote 0; Python expectEq failed (see stderr)"
    );
}

#[test]
fn luna_dict_yaml() {
    let ir = world_ir();
    let mut region = alloc_world(&ir);
    let bound = bind_store(&ir);
    bound.call("import_all", &mut region, 0).unwrap();
    bound.call("persist_encodings", &mut region, 0).unwrap();
    bound.call("check_dict", &mut region, 0).unwrap();
    assert_eq!(u64_at(&ir, &region, "mismatch_count"), 0);
}
