//! Packet workshop: prefix Frame/EthFrame, C+Python+Rust methods, C `>>` seal.
//! Feature `python`. Named gate: `packet_declared_dispatch`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use stardust::{
    BoundClass, CallRet, Class, Endianness, Import, Ir, Lang, Library, MethodLoader, Param, Path,
    Region, Ret, Scalar, StructBuilder, Type, Value,
};

const ETH: u64 = 16;
const DB: u64 = 96;
const KIND: u16 = 0x0800;

fn ptr_len_void(symbol: &str, lang: Lang) -> Import {
    Import::new(symbol, vec![Param::Ptr, Param::Len], Ret::Void).with_lang(lang)
}

fn frame_lay() -> Type {
    StructBuilder::new()
        .field("kind", Type::scalar(Scalar::U16))
        .field("length", Type::scalar(Scalar::U16))
        .field("checksum", Type::scalar(Scalar::U32))
        .field("note", Type::array(Type::scalar(Scalar::U8), 32).unwrap())
        .field("proto", Type::link())
        .repr_c()
        .unwrap()
}

fn eth_frame_lay() -> Type {
    StructBuilder::new()
        .field("kind", Type::scalar(Scalar::U16))
        .field("length", Type::scalar(Scalar::U16))
        .field("checksum", Type::scalar(Scalar::U32))
        .field("note", Type::array(Type::scalar(Scalar::U8), 32).unwrap())
        .field("proto", Type::link())
        .field("dst", Type::array(Type::scalar(Scalar::U8), 6).unwrap())
        .field("src", Type::array(Type::scalar(Scalar::U8), 6).unwrap())
        .repr_c()
        .unwrap()
}

fn protocol_lay() -> Type {
    let row = StructBuilder::new()
        .field("code", Type::scalar(Scalar::U16))
        .field("name", Type::array(Type::scalar(Scalar::U8), 8).unwrap())
        .repr_c()
        .unwrap();
    StructBuilder::new()
        .field("count", Type::scalar(Scalar::U32))
        .field("digest", Type::scalar(Scalar::U32))
        .field("rows", Type::array(row, 4).unwrap())
        .repr_c()
        .unwrap()
}

fn packet_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("FrameLay", frame_lay());
    ir.insert("EthFrameLay", eth_frame_lay());
    ir.insert("ProtocolLay", protocol_lay());

    let mut frame = Class {
        layout: "FrameLay".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    frame
        .insert_method("parse", ptr_len_void("parse_frame", Lang::C))
        .unwrap();
    frame
        .insert_method("explain", ptr_len_void("explain", Lang::Python))
        .unwrap();
    ir.insert_class("Frame", frame).unwrap();

    let mut eth = Class {
        layout: "EthFrameLay".into(),
        base: Some("Frame".into()),
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    eth.insert_method("parse", ptr_len_void("parse_eth", Lang::Rust))
        .unwrap();
    ir.insert_class("EthFrame", eth).unwrap();

    let mut db = Class {
        layout: "ProtocolLay".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    db.insert_export_method("seal", ptr_len_void("seal_db", Lang::C))
        .unwrap();
    ir.insert_class("ProtocolDb", db).unwrap();
    ir
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn frame_sd() -> PathBuf {
    root().join("examples/packet/frame.sd")
}

fn explain_py() -> PathBuf {
    root().join("examples/packet/explain.py")
}

fn checksum_skip4(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .enumerate()
        .filter(|(i, _)| !(4..8).contains(i))
        .fold(0u32, |acc, (_, b)| acc.wrapping_add(u32::from(*b)))
}

fn u32_at(region: &Region, off: u64) -> u32 {
    let i = usize::try_from(off).unwrap();
    u32::from_le_bytes(region.as_bytes()[i..i + 4].try_into().unwrap())
}

fn store_u8_array(
    ir: &Ir,
    region: &mut Region,
    layout: &str,
    offset: u64,
    field: &str,
    bytes: &[u8],
) {
    ir.store_at(
        layout,
        region,
        offset,
        &Path::new().name(field),
        &Value::Array(bytes.iter().copied().map(Value::U8).collect()),
    )
    .unwrap();
}

#[test]
fn dump_frame_sd_if_env() {
    if std::env::var("STARDUST_DUMP_FRAME_SD").as_deref() != Ok("1") {
        return;
    }
    std::fs::write(frame_sd(), packet_ir().to_sd().unwrap()).unwrap();
}

#[test]
fn packet_declared_dispatch() {
    let ir = packet_ir();
    assert!(
        ir.imports.is_empty(),
        "class methods live on the class, not Ir.imports"
    );
    let on_disk = Ir::from_sd_path(frame_sd()).expect("examples/packet/frame.sd");
    assert_eq!(
        ir, on_disk,
        "dumpable IR is authority; frame.sd is the copy"
    );

    let frame_size = ir.layout("FrameLay").unwrap().inline_size();
    let eth_size = ir.layout("EthFrameLay").unwrap().inline_size();
    let db_size = ir.layout("ProtocolLay").unwrap().inline_size();
    assert_eq!(frame_size, 48);
    assert_eq!(eth_size, 64);
    assert_eq!(db_size, 48);

    let c_lib = Library::load(stardust_packet::c_lib_path()).expect("load packet C");
    let rs_lib = Library::load(stardust_packet::rs_lib_path()).expect("load packet Rust");
    let script = explain_py();

    let mut frame_loaders = BTreeMap::new();
    frame_loaders.insert("parse".into(), MethodLoader::Library(&c_lib));
    frame_loaders.insert(
        "explain".into(),
        MethodLoader::Python {
            script: script.clone(),
        },
    );
    let frame_bound = BoundClass::bind(&ir, "Frame", &frame_loaders).unwrap();

    let mut eth_loaders = BTreeMap::new();
    eth_loaders.insert("parse".into(), MethodLoader::Library(&rs_lib));
    eth_loaders.insert(
        "explain".into(),
        MethodLoader::Python {
            script: script.clone(),
        },
    );
    let eth_bound = BoundClass::bind(&ir, "EthFrame", &eth_loaders).unwrap();

    let mut db_loaders = BTreeMap::new();
    db_loaders.insert("seal".into(), MethodLoader::Library(&c_lib));
    let db_bound = BoundClass::bind(&ir, "ProtocolDb", &db_loaders).unwrap();

    let mut region = Region::allocate("packet", 256, 8).unwrap();
    for (i, b) in region.as_bytes_mut().iter_mut().enumerate() {
        *b = 0x33u8.wrapping_add(i as u8);
    }
    let eth_start = usize::try_from(ETH).unwrap();
    let eth_end = eth_start + usize::try_from(eth_size).unwrap();
    let db_start = usize::try_from(DB).unwrap();
    let db_end = db_start + usize::try_from(db_size).unwrap();
    region.as_bytes_mut()[eth_start..eth_end].fill(0);
    region.as_bytes_mut()[db_start..db_end].fill(0);

    ir.store_at(
        "EthFrameLay",
        &mut region,
        ETH,
        &Path::new().name("kind"),
        &Value::U16(KIND),
    )
    .unwrap();
    ir.store_at(
        "EthFrameLay",
        &mut region,
        ETH,
        &Path::new().name("length"),
        &Value::U16(u16::try_from(eth_size).unwrap()),
    )
    .unwrap();
    let dst = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
    let src = [0x11, 0x12, 0x13, 0x14, 0x15, 0x16];
    store_u8_array(&ir, &mut region, "EthFrameLay", ETH, "dst", &dst);
    store_u8_array(&ir, &mut region, "EthFrameLay", ETH, "src", &src);

    ir.store_at(
        "ProtocolLay",
        &mut region,
        DB,
        &Path::new().name("count"),
        &Value::U32(2),
    )
    .unwrap();
    ir.store_at(
        "ProtocolLay",
        &mut region,
        DB,
        &Path::new().name("rows").index(0).name("code"),
        &Value::U16(0x0800),
    )
    .unwrap();
    ir.store_at(
        "ProtocolLay",
        &mut region,
        DB,
        &Path::new().name("rows").index(0).name("name"),
        &Value::Array(b"IPv4\0\0\0\0".iter().copied().map(Value::U8).collect()),
    )
    .unwrap();
    ir.store_at(
        "ProtocolLay",
        &mut region,
        DB,
        &Path::new().name("rows").index(1).name("code"),
        &Value::U16(0x86DD),
    )
    .unwrap();
    ir.store_at(
        "ProtocolLay",
        &mut region,
        DB,
        &Path::new().name("rows").index(1).name("name"),
        &Value::Array(b"IPv6\0\0\0\0".iter().copied().map(Value::U8).collect()),
    )
    .unwrap();

    let seed = region.as_bytes().to_vec();
    let base = region.as_ptr();

    let mut eth = eth_bound.handle(ETH);
    let dbh = db_bound.handle(DB);
    eth.link("proto", &dbh, &mut region).unwrap();
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through link"
    );
    assert_eq!(
        ir.retrieve_at("EthFrameLay", &region, ETH, &Path::new().name("proto"))
            .unwrap(),
        Value::U64(DB)
    );

    let ret_seal = eth.call_export("seal", &mut region).unwrap();
    assert_eq!(ret_seal, CallRet::Void);
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through seal"
    );
    let expected_seal = checksum_skip4(&region.as_bytes()[db_start..db_end]);
    assert_eq!(u32_at(&region, DB + 4), expected_seal);
    assert_ne!(expected_seal, 0, "protocol table must contribute to seal");

    let ret_rust = eth_bound.call("parse", &mut region, ETH).unwrap();
    assert_eq!(ret_rust, CallRet::Void);
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through Rust parse"
    );
    let rust_sum = u32_at(&region, ETH + 4);
    let expected_eth = checksum_skip4(&region.as_bytes()[eth_start..eth_end]);
    assert_eq!(rust_sum, expected_eth);

    let mac_before_c = region.as_bytes()[eth_start + 48..eth_start + 60].to_vec();
    let ret_c = frame_bound.call("parse", &mut region, ETH).unwrap();
    assert_eq!(ret_c, CallRet::Void);
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through C parse"
    );
    let c_sum = u32_at(&region, ETH + 4);
    let expected_frame = checksum_skip4(&region.as_bytes()[eth_start..eth_start + 48]);
    assert_eq!(c_sum, expected_frame);
    assert_ne!(
        rust_sum, c_sum,
        "Rust checksum must include MAC bytes the C Frame view does not"
    );
    assert_eq!(
        &region.as_bytes()[eth_start + 48..eth_start + 60],
        mac_before_c.as_slice(),
        "Frame parse must leave dst/src unchanged"
    );

    let ret_py = frame_bound.call("explain", &mut region, ETH).unwrap();
    assert_eq!(ret_py, CallRet::Void);
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through Python explain"
    );
    assert_eq!(&region.as_bytes()[eth_start + 8..eth_start + 12], b"IPv4");
    assert!(
        region.as_bytes()[eth_start + 12..eth_start + 40]
            .iter()
            .all(|&b| b == 0),
        "note tail must be zeros"
    );
    assert_eq!(
        u32_at(&region, ETH + 4),
        c_sum,
        "explain must not rewrite checksum"
    );

    let after = region.as_bytes();
    assert_eq!(
        &after[..eth_start],
        &seed[..eth_start],
        "bytes before the instance must be unchanged"
    );
    assert_eq!(
        &after[eth_end..db_start],
        &seed[eth_end..db_start],
        "gap between instance and protocol table must be unchanged"
    );
    assert_eq!(
        &after[db_end..],
        &seed[db_end..],
        "bytes after the protocol table must be unchanged"
    );
    assert_eq!(&after[eth_start + 48..eth_start + 54], &dst);
    assert_eq!(&after[eth_start + 54..eth_start + 60], &src);

    let db_after = &after[db_start..db_end];
    let mut db_seed = seed[db_start..db_end].to_vec();
    db_seed[4..8].copy_from_slice(&expected_seal.to_le_bytes());
    assert_eq!(
        db_after,
        db_seed.as_slice(),
        "seal must change only ProtocolLay.digest"
    );
}
