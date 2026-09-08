use std::path::PathBuf;

use stardust::{
    BitOrder, BitpackBuilder, Bump, CallArg, Class, ColumnarBuilder, Endianness, Error, Field,
    Import, Ir, Lang, Library, Param, Path, Region, Ret, Scalar, StructBuilder, StructType, Type,
    Value,
};

// L1
fn demo_ir() -> Ir {
    let ty = StructBuilder::new()
        .field("tag", Type::scalar(Scalar::I32))
        .field("ratio", Type::scalar(Scalar::F64))
        .field("arr", Type::array(Type::scalar(Scalar::U16), 3).unwrap())
        .field("xs", Type::slice(Type::scalar(Scalar::U32)))
        .repr_c()
        .unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Demo", ty);
    ir
}

fn payload_offset(ir: &Ir) -> u64 {
    ir.layout("Demo").unwrap().inline_size()
}

fn store_fixture(ir: &Ir, region: &mut Region) {
    ir.store("Demo", region, &Path::new().name("tag"), &Value::I32(-7))
        .unwrap();
    ir.store(
        "Demo",
        region,
        &Path::new().name("ratio"),
        &Value::F64(1.25),
    )
    .unwrap();
    ir.store(
        "Demo",
        region,
        &Path::new().name("arr"),
        &Value::Array(vec![Value::U16(1), Value::U16(2), Value::U16(3)]),
    )
    .unwrap();
    ir.store(
        "Demo",
        region,
        &Path::new().name("xs"),
        &Value::Slice {
            offset: payload_offset(ir),
            elems: vec![Value::U32(10), Value::U32(20)],
        },
    )
    .unwrap();
}

#[test]
fn round_trip_ir() {
    let inner = StructBuilder::new()
        .field("a", Type::scalar(Scalar::U8))
        .field("b", Type::scalar(Scalar::I16))
        .repr_c()
        .unwrap();
    let root = StructBuilder::new()
        .field("inner", inner)
        .field("n", Type::scalar(Scalar::U64))
        .field("arr", Type::array(Type::scalar(Scalar::F32), 2).unwrap())
        .field("tail", Type::slice(Type::scalar(Scalar::I32)))
        .repr_c()
        .unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Root", root);
    ir.insert("Unit", Type::scalar(Scalar::U8));

    let json = ir.to_json().unwrap();
    let reloaded = Ir::from_json(&json).unwrap();
    assert_eq!(reloaded, ir);
}

#[test]
fn store_retrieve() {
    let ir = demo_ir();
    let mut region = Region::allocate("demo", 64, 8).unwrap();
    store_fixture(&ir, &mut region);

    assert_eq!(
        ir.retrieve("Demo", &region, &Path::new().name("tag"))
            .unwrap(),
        Value::I32(-7)
    );
    assert_eq!(
        ir.retrieve("Demo", &region, &Path::new().name("ratio"))
            .unwrap(),
        Value::F64(1.25)
    );
    assert_eq!(
        ir.retrieve("Demo", &region, &Path::new().name("arr"))
            .unwrap(),
        Value::Array(vec![Value::U16(1), Value::U16(2), Value::U16(3)])
    );
    assert_eq!(
        ir.retrieve("Demo", &region, &Path::new().name("arr").index(1))
            .unwrap(),
        Value::U16(2)
    );
    assert_eq!(
        ir.retrieve("Demo", &region, &Path::new().name("xs"))
            .unwrap(),
        Value::Slice {
            offset: payload_offset(&ir),
            elems: vec![Value::U32(10), Value::U32(20)],
        }
    );
    assert_eq!(
        ir.retrieve("Demo", &region, &Path::new().name("xs").index(0))
            .unwrap(),
        Value::U32(10)
    );
}

#[test]
fn view_identity() {
    let ir = demo_ir();
    let mut region = Region::allocate("demo", 64, 8).unwrap();
    store_fixture(&ir, &mut region);

    let tag_path = Path::new().name("tag");
    let ratio_path = Path::new().name("ratio");
    let xs_path = Path::new().name("xs").index(1);

    let via_retrieve_tag = ir.retrieve("Demo", &region, &tag_path).unwrap();
    let via_retrieve_ratio = ir.retrieve("Demo", &region, &ratio_path).unwrap();
    let via_retrieve_xs = ir.retrieve("Demo", &region, &xs_path).unwrap();
    let base = region.as_ptr();
    {
        let view = ir.view("Demo", &mut region).unwrap();
        assert_eq!(view.as_ptr(), base);
        assert_eq!(view.get(&tag_path).unwrap(), via_retrieve_tag);
        assert_eq!(view.get(&ratio_path).unwrap(), via_retrieve_ratio);
        assert_eq!(view.get(&xs_path).unwrap(), via_retrieve_xs);
    }

    {
        let mut view = ir.view("Demo", &mut region).unwrap();
        view.set(&tag_path, &Value::I32(99)).unwrap();
        view.set(&xs_path, &Value::U32(77)).unwrap();
    }
    assert_eq!(
        ir.retrieve("Demo", &region, &tag_path).unwrap(),
        Value::I32(99)
    );
    assert_eq!(
        ir.retrieve("Demo", &region, &xs_path).unwrap(),
        Value::U32(77)
    );

    ir.store("Demo", &mut region, &ratio_path, &Value::F64(3.5))
        .unwrap();
    {
        let view = ir.view("Demo", &mut region).unwrap();
        assert_eq!(view.get(&ratio_path).unwrap(), Value::F64(3.5));
        assert_eq!(view.get(&tag_path).unwrap(), Value::I32(99));
    }
}

#[test]
fn alignment() {
    let align = 64usize;
    let region = Region::allocate("aligned", 128, align).unwrap();
    let base = region.as_ptr() as usize;
    assert_eq!(base % align, 0);
    assert_eq!(region.align(), align);
}

// L2
fn sample_ir() -> Ir {
    let inner = StructBuilder::new()
        .field("a", Type::scalar(Scalar::U8))
        .field("b", Type::scalar(Scalar::I16))
        .repr_c()
        .unwrap();
    let demo = StructBuilder::new()
        .field("inner", inner)
        .field("tag", Type::scalar(Scalar::I32))
        .field("arr", Type::array(Type::scalar(Scalar::U16), 3).unwrap())
        .field("xs", Type::slice(Type::scalar(Scalar::U32)))
        .repr_c()
        .unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Demo", demo);
    ir.insert_import(
        "process",
        Import::new("process", vec![Param::Ptr, Param::Len], Ret::Void),
    )
    .unwrap();
    ir
}

fn load_fixture() -> Library {
    Library::load(stardust_c::process_lib_path()).expect("load W2 C fixture")
}

fn fill_non_a1(region: &mut Region) {
    for (i, b) in region.as_bytes_mut().iter_mut().enumerate() {
        *b = 0x33u8.wrapping_add(i as u8);
        if *b == 0xA1 {
            *b = 0x33;
        }
    }
}

const OFFSET: u64 = 8;
const LEN: u64 = 16;
const SPAN: std::ops::Range<usize> = 8..24;

#[test]
fn ir_equality() {
    let ir = sample_ir();
    let sd = ir.to_sd().expect("to_sd");
    let reloaded = Ir::from_sd(&sd).expect("from_sd");
    assert_eq!(reloaded, ir, "parse(print(ir)) must equal ir (PartialEq)");
}

#[test]
fn builder_eq_file() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/w3.sd");
    let ir = Ir::from_sd_path(&path).expect("parse tests/data/w3.sd");
    let lib = load_fixture();
    let func_a = lib.bind(&ir, "write_a").unwrap();
    let func_b = lib.bind(&ir, "mut_b").unwrap();

    let mut region = Region::allocate("w3", 32, 8).unwrap();
    fill_non_a1(&mut region);
    let before = region.as_bytes().to_vec();

    let before_a = region.as_ptr();
    func_a
        .call(
            &mut region,
            &[
                CallArg::Ptr {
                    region: 0,
                    offset: OFFSET,
                },
                CallArg::Len(LEN),
            ],
        )
        .unwrap();
    let after_a = region.as_ptr();
    func_b
        .call(
            &mut region,
            &[
                CallArg::Ptr {
                    region: 0,
                    offset: OFFSET,
                },
                CallArg::Len(LEN),
            ],
        )
        .unwrap();
    let after_b = region.as_ptr();

    assert_eq!(
        before_a, after_a,
        "as_ptr() must be unchanged through write_a"
    );
    assert_eq!(after_a, after_b, "as_ptr() must be unchanged through mut_b");
    assert_eq!(before_a, after_b, "A and B must share the same region base");

    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    assert!(
        after[SPAN].iter().all(|&b| b == 0xA2),
        "span [8,24) must be 0xA2 after write_a then mut_b from w3.sd, got {:?}",
        &after[SPAN]
    );
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");
}

// L3
fn table_ir(capacity: u64) -> Ir {
    let table = ColumnarBuilder::new(capacity)
        .column("x", Type::scalar(Scalar::U32))
        .unwrap()
        .column("ys", Type::array(Type::scalar(Scalar::U16), 3).unwrap())
        .unwrap()
        .build()
        .unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Table", table);
    ir
}

#[test]
fn columnar_ir() {
    let mut ir = table_ir(16);
    let aos = StructBuilder::new()
        .field("tag", Type::scalar(Scalar::I32))
        .repr_c()
        .unwrap();
    ir.insert("Demo", aos);

    let json = ir.to_json().unwrap();
    let reloaded = Ir::from_json(&json).unwrap();
    assert_eq!(reloaded, ir);

    let sd = ir.to_sd().expect("to_sd");
    let from_sd = Ir::from_sd(&sd).expect("from_sd");
    assert_eq!(from_sd, ir, "from_sd(to_sd(ir)) must equal ir (PartialEq)");
}

#[test]
fn soa_view() {
    let ir = table_ir(8);
    let layout = match ir.layout("Table").unwrap() {
        Type::Columnar(c) => c,
        other => panic!("expected columnar, got {other:?}"),
    };
    let x_col = layout
        .columns
        .iter()
        .find(|f| f.name == "x")
        .expect("column x");
    let ys_col = layout
        .columns
        .iter()
        .find(|f| f.name == "ys")
        .expect("column ys");

    let size = layout.size as usize;
    let align = layout.align as usize;
    let mut region = Region::allocate("soa", size, align).unwrap();

    {
        let bytes = region.as_bytes_mut();
        for i in 4..8 {
            let off = x_col.offset as usize + i * 4;
            bytes[off..off + 4].fill(0xAB);
            let off = ys_col.offset as usize + i * 6;
            bytes[off..off + 6].fill(0xCD);
        }
    }

    let base = region.as_ptr();
    {
        let mut cv = ir.column_view("Table", &mut region).unwrap();
        assert_eq!(cv.row_count(), 0);
        cv.set_row_count(4).unwrap();
        assert_eq!(cv.row_count(), 4);
        assert!(cv.set_row_count(9).is_err());
        assert_eq!(cv.row_count(), 4);

        let xs = cv.column_mut::<u32>("x").unwrap();
        assert_eq!(xs.len(), 4);
        assert_eq!(xs.as_ptr() as *const u8, unsafe {
            base.add(x_col.offset as usize)
        });
        xs.copy_from_slice(&[10, 20, 30, 40]);

        let ys = cv.column_mut::<[u16; 3]>("ys").unwrap();
        assert_eq!(ys.len(), 4);
        assert_eq!(ys.as_ptr() as *const u8, unsafe {
            base.add(ys_col.offset as usize)
        });
        ys.copy_from_slice(&[[1, 2, 3], [4, 5, 6], [7, 8, 9], [10, 11, 12]]);

        let xs = cv.column_mut::<u32>("x").unwrap();
        xs[1] = 99;
    }

    {
        let mut cv = ir.column_view("Table", &mut region).unwrap();
        assert_eq!(cv.row_count(), 4);
        let xs = cv.column_mut::<u32>("x").unwrap();
        assert_eq!(xs, &[10, 99, 30, 40]);
        assert_eq!(xs.as_ptr() as *const u8, unsafe {
            base.add(x_col.offset as usize)
        });

        let ys = cv.column_mut::<[u16; 3]>("ys").unwrap();
        assert_eq!(ys, &[[1, 2, 3], [4, 5, 6], [7, 8, 9], [10, 11, 12]]);

        assert_eq!(cv.column_mut::<u16>("ys").unwrap_err(), Error::TypeMismatch);
        assert_eq!(
            cv.column_mut::<u32>("nope").unwrap_err(),
            Error::UnknownPath
        );
    }

    {
        let bytes = region.as_bytes();
        for i in 4..8 {
            let off = x_col.offset as usize + i * 4;
            assert_eq!(
                &bytes[off..off + 4],
                &[0xAB; 4],
                "x capacity tail must stay"
            );
            let off = ys_col.offset as usize + i * 6;
            assert_eq!(
                &bytes[off..off + 6],
                &[0xCD; 6],
                "ys capacity tail must stay"
            );
        }
    }

    assert_eq!(
        ir.retrieve("Table", &region, &Path::new()).unwrap_err(),
        Error::TypeMismatch
    );
    assert_eq!(
        ir.store("Table", &mut region, &Path::new(), &Value::U32(1))
            .unwrap_err(),
        Error::TypeMismatch
    );
    assert_eq!(
        ir.view("Table", &mut region).err(),
        Some(Error::TypeMismatch)
    );
}

// L4
fn node_ty() -> Type {
    StructBuilder::new()
        .field("tag", Type::scalar(Scalar::U8))
        .field("next", Type::link())
        .repr_c()
        .unwrap()
}

fn node_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Node", node_ty());
    ir
}

fn walk_ir() -> Ir {
    let mut ir = node_ir();
    ir.insert_import(
        "arena_walk",
        Import::new("arena_walk", vec![Param::Ptr, Param::Len], Ret::Void),
    )
    .unwrap();
    ir
}

#[test]
fn offset_cycle() {
    let ir = node_ir();
    let layout = ir.layout("Node").unwrap();
    assert_eq!(layout.inline_size(), 16);
    assert_eq!(layout.align(), 8);

    let json = ir.to_json().unwrap();
    assert!(
        json.contains("\"Link\""),
        "JSON unit for Type::Link must be \"Link\", got {json}"
    );
    let reloaded = Ir::from_json(&json).unwrap();
    assert_eq!(reloaded, ir);

    let sd = ir.to_sd().expect("to_sd");
    assert!(sd.contains("link"), ".sd must print keyword link, got {sd}");
    let from_sd = Ir::from_sd(&sd).expect("from_sd");
    assert_eq!(from_sd, ir, "from_sd(to_sd(ir)) must equal ir (PartialEq)");

    let mut region = Region::allocate("w13", 64, 8).unwrap();
    let (a, b) = {
        let mut bump = Bump::new(&mut region);
        let a = bump.alloc(layout).unwrap();
        let b = bump.alloc(layout).unwrap();
        (a, b)
    };
    assert_eq!(a, 8, "first object is at offset 8");
    assert_eq!(b, 24);

    assert_eq!(
        ir.retrieve_at("Node", &region, a, &Path::new().name("next"))
            .unwrap(),
        Value::U64(0),
        "unset link is null 0"
    );

    ir.store_at(
        "Node",
        &mut region,
        a,
        &Path::new().name("tag"),
        &Value::U8(0xA1),
    )
    .unwrap();
    ir.store_at(
        "Node",
        &mut region,
        a,
        &Path::new().name("next"),
        &Value::U64(b),
    )
    .unwrap();
    ir.store_at(
        "Node",
        &mut region,
        b,
        &Path::new().name("tag"),
        &Value::U8(0xA1),
    )
    .unwrap();
    ir.store_at(
        "Node",
        &mut region,
        b,
        &Path::new().name("next"),
        &Value::U64(a),
    )
    .unwrap();

    assert_eq!(
        ir.retrieve_at("Node", &region, a, &Path::new().name("next"))
            .unwrap(),
        Value::U64(b)
    );
    assert_eq!(
        ir.retrieve_at("Node", &region, b, &Path::new().name("next"))
            .unwrap(),
        Value::U64(a)
    );
    assert_eq!(
        ir.retrieve_at("Node", &region, a, &Path::new().name("tag"))
            .unwrap(),
        Value::U8(0xA1)
    );
}

#[test]
fn arena_c_walk() {
    let ir = walk_ir();
    let layout = ir.layout("Node").unwrap();
    let lib = load_fixture();
    let walk = lib.bind(&ir, "arena_walk").unwrap();

    let mut region = Region::allocate("w13", 64, 8).unwrap();
    for (i, byte) in region.as_bytes_mut()[..8].iter_mut().enumerate() {
        *byte = 0x50u8.wrapping_add(i as u8);
    }

    let (a, b) = {
        let mut bump = Bump::new(&mut region);
        let a = bump.alloc(layout).unwrap();
        let b = bump.alloc(layout).unwrap();
        (a, b)
    };
    assert_eq!(a, 8);
    assert_eq!(b, 24);

    ir.store_at(
        "Node",
        &mut region,
        a,
        &Path::new().name("tag"),
        &Value::U8(0xA1),
    )
    .unwrap();
    ir.store_at(
        "Node",
        &mut region,
        a,
        &Path::new().name("next"),
        &Value::U64(b),
    )
    .unwrap();
    ir.store_at(
        "Node",
        &mut region,
        b,
        &Path::new().name("tag"),
        &Value::U8(0xA1),
    )
    .unwrap();
    ir.store_at(
        "Node",
        &mut region,
        b,
        &Path::new().name("next"),
        &Value::U64(a),
    )
    .unwrap();

    let prefix = region.as_bytes()[..8].to_vec();
    let base = region.as_ptr();
    let size = region.size() as u64;

    walk.call(
        &mut region,
        &[
            CallArg::Ptr {
                region: 0,
                offset: 0,
            },
            CallArg::Len(size),
        ],
    )
    .unwrap();

    assert_eq!(region.as_ptr(), base, "as_ptr() identity must be unchanged");
    assert_eq!(
        &region.as_bytes()[..8],
        prefix.as_slice(),
        "prefix [0,8) must be unchanged"
    );

    assert_eq!(
        ir.retrieve_at("Node", &region, a, &Path::new().name("tag"))
            .unwrap(),
        Value::U8(0xA2)
    );
    assert_eq!(
        ir.retrieve_at("Node", &region, b, &Path::new().name("tag"))
            .unwrap(),
        Value::U8(0xA2)
    );
    assert_eq!(
        ir.retrieve_at("Node", &region, a, &Path::new().name("next"))
            .unwrap(),
        Value::U64(b),
        "stored links must stay offsets"
    );
    assert_eq!(
        ir.retrieve_at("Node", &region, b, &Path::new().name("next"))
            .unwrap(),
        Value::U64(a),
        "stored links must stay offsets"
    );
    let as_ptr_u64 = base as u64;
    assert_ne!(
        ir.retrieve_at("Node", &region, a, &Path::new().name("next"))
            .unwrap(),
        Value::U64(as_ptr_u64),
        "links must not be rewritten to process pointers"
    );
    assert_ne!(
        ir.retrieve_at("Node", &region, b, &Path::new().name("next"))
            .unwrap(),
        Value::U64(as_ptr_u64)
    );
}

// L5
fn flags_ty() -> Type {
    BitpackBuilder::new(Scalar::U32)
        .field("a", 3)
        .field("b", 5)
        .build()
        .unwrap()
}

fn flags_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Flags", flags_ty());
    ir
}

fn process_ir() -> Ir {
    let mut ir = flags_ir();
    ir.insert_import(
        "process",
        Import::new("process", vec![Param::Ptr, Param::Len], Ret::Void),
    )
    .unwrap();
    ir
}

#[test]
fn bitpack_ir() {
    let ir = flags_ir();
    assert_eq!(ir.bit_order, BitOrder::Lsb0);
    let layout = ir.layout("Flags").unwrap();
    assert_eq!(layout.inline_size(), 4);
    assert_eq!(layout.align(), 4);
    match layout {
        Type::Bitpack(bp) => {
            assert_eq!(bp.container, Some(Scalar::U32));
            assert_eq!(bp.span, 4);
            assert_eq!(bp.fields.len(), 2);
            assert_eq!(bp.fields[0].name, "a");
            assert_eq!(bp.fields[0].width, 3);
            assert_eq!(bp.fields[0].bit_offset, 0);
            assert_eq!(bp.fields[1].name, "b");
            assert_eq!(bp.fields[1].width, 5);
            assert_eq!(bp.fields[1].bit_offset, 3);
        }
        other => panic!("expected Bitpack, got {other:?}"),
    }

    let json = ir.to_json().unwrap();
    let reloaded = Ir::from_json(&json).unwrap();
    assert_eq!(reloaded, ir);

    let without_bit_order = json.replacen(",\"bit_order\":\"Lsb0\"", "", 1);
    let from_default = Ir::from_json(&without_bit_order).unwrap_or_else(|_| {
        let stripped = json.replacen("\"bit_order\":\"Lsb0\",", "", 1);
        Ir::from_json(&stripped).expect("JSON without bit_order must parse as Lsb0")
    });
    assert_eq!(from_default.bit_order, BitOrder::Lsb0);
    assert_eq!(from_default, ir);

    let sd = ir.to_sd().expect("to_sd");
    assert!(
        sd.contains("bit_order lsb0"),
        "to_sd must print bit_order, got {sd}"
    );
    assert!(
        sd.contains("bitpack u32"),
        ".sd must print bitpack sugar, got {sd}"
    );
    let from_sd = Ir::from_sd(&sd).expect("from_sd");
    assert_eq!(from_sd, ir, "from_sd(to_sd(ir)) must equal ir (PartialEq)");

    let msb0_sd =
        "endian little\nbit_order msb0\nlayout Flags bitpack u32 {\n  a: 1;\n  b: 3;\n}\n";
    let err = Ir::from_sd(msb0_sd).unwrap_err();
    assert!(
        matches!(err, Error::UnsupportedBitOrder),
        "declaring msb0 must be UnsupportedBitOrder, got {err:?}"
    );
    assert!(
        err.to_string().contains("msb0"),
        "UnsupportedBitOrder Display must name msb0, got {err}"
    );

    let mut msb0 = flags_ir();
    msb0.bit_order = BitOrder::Msb0;
    let mut region = Region::allocate("w14-msb0", 4, 4).unwrap();
    let view_err = match msb0.view("Flags", &mut region) {
        Ok(_) => panic!("msb0 view must be Err"),
        Err(e) => e,
    };
    assert!(
        matches!(view_err, Error::UnsupportedBitOrder),
        "msb0 view must be Err, got {view_err:?}"
    );
    let get_err = msb0
        .retrieve("Flags", &region, &Path::new().name("a"))
        .unwrap_err();
    assert!(
        matches!(get_err, Error::UnsupportedBitOrder),
        "msb0 retrieve must be Err, got {get_err:?}"
    );

    let packed = flags_ty();
    assert!(
        StructBuilder::new()
            .field("flags", packed.clone())
            .repr_c()
            .is_err(),
        "Bitpack is not a repr_c field type"
    );
    assert!(
        ColumnarBuilder::new(1).column("flags", packed).is_err(),
        "Bitpack is not a SoA column"
    );
}

#[test]
fn bitpack_view() {
    let ir = process_ir();
    let layout = ir.layout("Flags").unwrap();
    let size = layout.inline_size() as usize;
    let align = layout.align() as usize;
    assert_eq!(size, 4);

    let mut region = Region::allocate("w14", size, align).unwrap();
    assert_eq!(region.size(), size);
    let base = region.as_ptr();

    {
        let mut view = ir.view("Flags", &mut region).unwrap();
        assert_eq!(view.as_ptr(), base, "view.as_ptr() == region.as_ptr()");
        view.set(&Path::new().name("a"), &Value::U64(5)).unwrap();
        view.set(&Path::new().name("b"), &Value::U64(17)).unwrap();
        assert_eq!(view.get(&Path::new().name("a")).unwrap(), Value::U64(5));
        assert_eq!(view.get(&Path::new().name("b")).unwrap(), Value::U64(17));
        assert!(
            view.set(&Path::new().name("a"), &Value::U64(8)).is_err(),
            "value must fit in declared width"
        );
        assert_eq!(
            view.get(&Path::new()).unwrap_err(),
            Error::TypeMismatch,
            "named fields are the API"
        );
        assert_eq!(view.as_ptr(), base);
    }

    assert_eq!(
        ir.retrieve("Flags", &region, &Path::new().name("a"))
            .unwrap(),
        Value::U64(5)
    );
    assert_eq!(
        ir.retrieve("Flags", &region, &Path::new().name("b"))
            .unwrap(),
        Value::U64(17)
    );
    // lsb0: a=5 at bits 0..3, b=17 at bits 3..8 → 5 | (17 << 3) = 141 = 0x8D
    assert_eq!(
        &region.as_bytes()[..4],
        &[0x8D, 0x00, 0x00, 0x00],
        "retrieval must see packed bytes, not exploded scalars"
    );

    let lib = load_fixture();
    let process = lib.bind(&ir, "process").unwrap();
    process
        .call(
            &mut region,
            &[
                CallArg::Ptr {
                    region: 0,
                    offset: 0,
                },
                CallArg::Len(size as u64),
            ],
        )
        .unwrap();

    assert_eq!(region.as_ptr(), base, "as_ptr() identity must be unchanged");
    assert_eq!(
        &region.as_bytes()[..4],
        &[0x8E, 0x01, 0x01, 0x01],
        "C process wrapping +1 must see packed container bytes"
    );
    assert_eq!(
        ir.retrieve("Flags", &region, &Path::new().name("a"))
            .unwrap(),
        Value::U64(6)
    );
    assert_eq!(
        ir.retrieve("Flags", &region, &Path::new().name("b"))
            .unwrap(),
        Value::U64(17),
        "C must not unpack into host scalars"
    );
}

fn frame_ty() -> Type {
    BitpackBuilder::span(3)
        .unwrap()
        .field("a", 12)
        .field("b", 12)
        .build()
        .unwrap()
}

fn frame_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Frame", frame_ty());
    ir
}

#[test]
fn bitstream_ir() {
    let ir = frame_ir();
    let layout = ir.layout("Frame").unwrap();
    assert_eq!(layout.inline_size(), 3);
    assert_eq!(layout.align(), 1);
    match layout {
        Type::Bitpack(bp) => {
            assert_eq!(bp.container, None);
            assert_eq!(bp.span, 3);
            assert_eq!(bp.size, 3);
            assert_eq!(bp.align, 1);
            assert_eq!(bp.fields.len(), 2);
            assert_eq!(bp.fields[0].name, "a");
            assert_eq!(bp.fields[0].width, 12);
            assert_eq!(bp.fields[0].bit_offset, 0);
            assert_eq!(bp.fields[1].name, "b");
            assert_eq!(bp.fields[1].width, 12);
            assert_eq!(bp.fields[1].bit_offset, 12);
        }
        other => panic!("expected Bitpack, got {other:?}"),
    }

    let json = ir.to_json().unwrap();
    assert!(
        json.contains("\"span\":3"),
        "L6 dump must include span, got {json}"
    );
    assert!(
        !json.contains("\"container\""),
        "L6 dump must omit container when None, got {json}"
    );
    let reloaded = Ir::from_json(&json).unwrap();
    assert_eq!(reloaded, ir);

    let sd = ir.to_sd().expect("to_sd");
    assert!(
        sd.contains("bitpack span 3"),
        ".sd must print bitpack span N, got {sd}"
    );
    let from_sd = Ir::from_sd(&sd).expect("from_sd");
    assert_eq!(from_sd, ir, "from_sd(to_sd(ir)) must equal ir (PartialEq)");

    let parsed =
        Ir::from_sd("endian little\nlayout Frame bitpack span 3 {\n  a: 12;\n  b: 12;\n}\n")
            .expect("bitpack span N");
    assert_eq!(parsed, ir);

    let mix = "endian little\nlayout Frame bitpack u32 span 5 {\n  a: 12;\n  b: 20;\n}\n";
    let mix_err = Ir::from_sd(mix).unwrap_err();
    assert!(
        matches!(mix_err, Error::InvalidLayout),
        "mix bitpack u32 span N must be InvalidLayout, got {mix_err:?}"
    );

    let mix_json = r#"{"endianness":"Little","layouts":{"Frame":{"Bitpack":{"container":"U32","span":5,"fields":[],"size":5,"align":1}}}}"#;
    assert!(
        Ir::from_json(mix_json).is_err(),
        "JSON mix container + span != container.size() must be Err"
    );

    let missing_span = r#"{"endianness":"Little","layouts":{"Frame":{"Bitpack":{"fields":[{"name":"a","width":12,"bit_offset":0}],"size":3,"align":1}}}}"#;
    assert!(
        Ir::from_json(missing_span).is_err(),
        "span-only JSON without span must be Err"
    );

    let wide_err = BitpackBuilder::span(16)
        .unwrap()
        .field("wide", 65)
        .build()
        .unwrap_err();
    assert!(
        matches!(wide_err, Error::Overflow),
        "width > 64 must be Overflow, got {wide_err:?}"
    );
    let wide_sd = "endian little\nlayout Frame bitpack span 16 {\n  wide: 65;\n}\n";
    let wide_sd_err = Ir::from_sd(wide_sd).unwrap_err();
    assert!(
        matches!(wide_sd_err, Error::Overflow),
        "DSL width > 64 must be Overflow, got {wide_sd_err:?}"
    );

    assert!(
        matches!(BitpackBuilder::span(0).unwrap_err(), Error::Overflow),
        "span 0 must be Overflow"
    );

    let msb0_sd =
        "endian little\nbit_order msb0\nlayout Frame bitpack span 3 {\n  a: 12;\n  b: 12;\n}\n";
    let err = Ir::from_sd(msb0_sd).unwrap_err();
    assert!(
        matches!(err, Error::UnsupportedBitOrder),
        "declaring msb0 must be UnsupportedBitOrder, got {err:?}"
    );

    let mut msb0 = frame_ir();
    msb0.bit_order = BitOrder::Msb0;
    let mut region = Region::allocate("l6-msb0", 3, 1).unwrap();
    let view_err = match msb0.view("Frame", &mut region) {
        Ok(_) => panic!("msb0 view must be Err"),
        Err(e) => e,
    };
    assert!(
        matches!(view_err, Error::UnsupportedBitOrder),
        "msb0 view must be Err, got {view_err:?}"
    );
}

#[test]
fn bitstream_view() {
    let mut ir = frame_ir();
    ir.insert_import(
        "process",
        Import::new("process", vec![Param::Ptr, Param::Len], Ret::Void),
    )
    .unwrap();
    let layout = ir.layout("Frame").unwrap();
    let size = layout.inline_size() as usize;
    let align = layout.align() as usize;
    assert_eq!(size, 3);
    assert_eq!(align, 1);

    let mut region = Region::allocate("l6", size, align).unwrap();
    assert_eq!(region.size(), size);
    let base = region.as_ptr();

    {
        let mut view = ir.view("Frame", &mut region).unwrap();
        assert_eq!(view.as_ptr(), base, "view.as_ptr() == region.as_ptr()");
        view.set(&Path::new().name("a"), &Value::U64(0xABC))
            .unwrap();
        view.set(&Path::new().name("b"), &Value::U64(0x123))
            .unwrap();
        assert_eq!(view.get(&Path::new().name("a")).unwrap(), Value::U64(0xABC));
        assert_eq!(view.get(&Path::new().name("b")).unwrap(), Value::U64(0x123));
        assert_eq!(view.as_ptr(), base);
    }

    assert_eq!(
        &region.as_bytes()[..3],
        &[0xBC, 0x3A, 0x12],
        "retrieval must see packed bytes, not exploded scalars"
    );
    assert_eq!(
        ir.retrieve("Frame", &region, &Path::new().name("a"))
            .unwrap(),
        Value::U64(0xABC)
    );
    assert_eq!(
        ir.retrieve("Frame", &region, &Path::new().name("b"))
            .unwrap(),
        Value::U64(0x123)
    );

    let lib = load_fixture();
    let process = lib.bind(&ir, "process").unwrap();
    process
        .call(
            &mut region,
            &[
                CallArg::Ptr {
                    region: 0,
                    offset: 0,
                },
                CallArg::Len(size as u64),
            ],
        )
        .unwrap();

    assert_eq!(region.as_ptr(), base, "as_ptr() identity must be unchanged");
    assert_eq!(
        &region.as_bytes()[..3],
        &[0xBD, 0x3B, 0x13],
        "C process wrapping +1 must see packed span bytes"
    );
    assert_eq!(
        ir.retrieve("Frame", &region, &Path::new().name("a"))
            .unwrap(),
        Value::U64(0xBBD)
    );
    assert_eq!(
        ir.retrieve("Frame", &region, &Path::new().name("b"))
            .unwrap(),
        Value::U64(0x133),
        "retrieve fields from packed bytes, not unpacked scalars"
    );
}

#[test]
fn class_ir() {
    let packet = StructBuilder::new()
        .field("n", Type::scalar(Scalar::U32))
        .repr_c()
        .unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Packet", packet);

    let mut widget = Class {
        layout: "Packet".into(),
        base: None,
        methods: Default::default(),
    };
    widget
        .insert_method(
            "bump",
            Import::new("bump_u8", vec![Param::Ptr, Param::Len], Ret::Void),
        )
        .unwrap();
    widget
        .insert_method(
            "scale",
            Import::new("scale", vec![Param::Ptr, Param::Len], Ret::Void).with_lang(Lang::Python),
        )
        .unwrap();
    ir.insert_class("Widget", widget).unwrap();

    let json = ir.to_json().unwrap();
    let reloaded = Ir::from_json(&json).unwrap();
    assert_eq!(reloaded, ir);

    let sd = ir.to_sd().expect("to_sd");
    let from_sd = Ir::from_sd(&sd).expect("from_sd");
    assert_eq!(from_sd, ir, "from_sd(to_sd(ir)) must equal ir (PartialEq)");

    let class_first = r#"endian little
class Widget {
  layout Packet;
  method bump(ptr, len) -> void {
    symbol bump_u8;
    lang c;
  }
  method scale(ptr, len) -> void {
    symbol scale;
    lang python;
  }
}
layout Packet repr_c {
  n: u32;
}
"#;
    assert_eq!(Ir::from_sd(class_first).unwrap(), ir);

    let mut missing = Ir::new(Endianness::Little);
    let err = missing
        .insert_class(
            "Widget",
            Class {
                layout: "Missing".into(),
                base: None,
                methods: Default::default(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::UnknownLayout));

    let json_missing = r#"{
        "endianness": "Little",
        "layouts": {},
        "classes": {
            "Widget": { "layout": "Missing", "methods": {} }
        }
    }"#;
    assert!(matches!(
        Ir::from_json(json_missing).unwrap_err(),
        Error::UnknownLayout
    ));

    let sd_missing = r#"endian little
class Widget {
  layout Missing;
}
"#;
    assert!(matches!(
        Ir::from_sd(sd_missing).unwrap_err(),
        Error::UnknownLayout
    ));

    let err = ir
        .insert_class(
            "Widget",
            Class {
                layout: "Packet".into(),
                base: None,
                methods: Default::default(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateClass));

    let sd_dup_class = r#"endian little
layout Packet repr_c {
  n: u32;
}
class Widget {
  layout Packet;
}
class Widget {
  layout Packet;
}
"#;
    assert!(matches!(
        Ir::from_sd(sd_dup_class).unwrap_err(),
        Error::DuplicateClass
    ));

    let mut methods = Class {
        layout: "Packet".into(),
        base: None,
        methods: Default::default(),
    };
    methods
        .insert_method(
            "bump",
            Import::new("bump_u8", vec![Param::Ptr, Param::Len], Ret::Void),
        )
        .unwrap();
    let err = methods
        .insert_method(
            "bump",
            Import::new("other", vec![Param::Ptr, Param::Len], Ret::Void),
        )
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateMethod));

    let sd_dup_method = r#"endian little
layout Packet repr_c {
  n: u32;
}
class Widget {
  layout Packet;
  method bump(ptr, len) -> void {
    symbol bump_u8;
    lang c;
  }
  method bump(ptr, len) -> void {
    symbol other;
    lang python;
  }
}
"#;
    assert!(matches!(
        Ir::from_sd(sd_dup_method).unwrap_err(),
        Error::DuplicateMethod
    ));
}

fn prefix_base_derived_layouts() -> (Type, Type) {
    let base = StructBuilder::new()
        .field("x", Type::scalar(Scalar::U32))
        .repr_c()
        .unwrap();
    let derived = StructBuilder::new()
        .field("x", Type::scalar(Scalar::U32))
        .field("y", Type::scalar(Scalar::U32))
        .repr_c()
        .unwrap();
    (base, derived)
}

fn prefix_builder_ir() -> Ir {
    let (base_ty, derived_ty) = prefix_base_derived_layouts();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("BaseLay", base_ty);
    ir.insert("DerivedLay", derived_ty);
    let mut base = Class {
        layout: "BaseLay".into(),
        base: None,
        methods: Default::default(),
    };
    base.insert_method(
        "paint",
        Import::new("write_a", vec![Param::Ptr, Param::Len], Ret::Void).with_lang(Lang::C),
    )
    .unwrap();
    ir.insert_class("Base", base).unwrap();
    let mut derived = Class {
        layout: "DerivedLay".into(),
        base: Some("Base".into()),
        methods: Default::default(),
    };
    derived
        .insert_method(
            "paint",
            Import::new("mut_b", vec![Param::Ptr, Param::Len], Ret::Void).with_lang(Lang::C),
        )
        .unwrap();
    ir.insert_class("Derived", derived).unwrap();
    ir
}

#[test]
fn prefix_ir() {
    let ir = prefix_builder_ir();
    assert_eq!(ir.layout("BaseLay").unwrap().inline_size(), 4);
    assert_eq!(ir.layout("DerivedLay").unwrap().inline_size(), 8);

    let json = ir.to_json().unwrap();
    let reloaded = Ir::from_json(&json).unwrap();
    assert_eq!(reloaded, ir);

    let sd = ir.to_sd().expect("to_sd");
    let from_sd = Ir::from_sd(&sd).expect("from_sd");
    assert_eq!(from_sd, ir, "from_sd(to_sd(ir)) must equal ir (PartialEq)");

    let class_before_base = r#"endian little
class Derived {
  layout DerivedLay;
  base Base;
  method paint(ptr, len) -> void {
    symbol mut_b;
    lang c;
  }
}
class Base {
  layout BaseLay;
  method paint(ptr, len) -> void {
    symbol write_a;
    lang c;
  }
}
layout BaseLay repr_c {
  x: u32;
}
layout DerivedLay repr_c {
  x: u32;
  y: u32;
}
"#;
    assert_eq!(Ir::from_sd(class_before_base).unwrap(), ir);

    let mut region = Region::allocate("o4-prefix", 16, 4).unwrap();
    ir.store(
        "DerivedLay",
        &mut region,
        &Path::new().name("x"),
        &Value::U32(0x1111_1111),
    )
    .unwrap();
    ir.store(
        "DerivedLay",
        &mut region,
        &Path::new().name("y"),
        &Value::U32(0x2222_2222),
    )
    .unwrap();
    assert_eq!(
        ir.retrieve("BaseLay", &region, &Path::new().name("x"))
            .unwrap(),
        Value::U32(0x1111_1111),
        "base view must read the derived prefix field at the same offset"
    );
    assert_eq!(
        ir.retrieve("DerivedLay", &region, &Path::new().name("x"))
            .unwrap(),
        ir.retrieve("BaseLay", &region, &Path::new().name("x"))
            .unwrap()
    );

    let mut missing = Ir::new(Endianness::Little);
    let (_, derived_ty) = prefix_base_derived_layouts();
    missing.insert("DerivedLay", derived_ty);
    let err = missing
        .insert_class(
            "Derived",
            Class {
                layout: "DerivedLay".into(),
                base: Some("Nope".into()),
                methods: Default::default(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::UnknownClass));

    let sd_unknown = r#"endian little
layout DerivedLay repr_c {
  x: u32;
  y: u32;
}
class Derived {
  layout DerivedLay;
  base Nope;
}
"#;
    assert!(matches!(
        Ir::from_sd(sd_unknown).unwrap_err(),
        Error::UnknownClass
    ));

    let sd_cycle = r#"endian little
layout ALay repr_c {
  x: u32;
}
layout BLay repr_c {
  x: u32;
}
class A {
  layout ALay;
  base B;
}
class B {
  layout BLay;
  base A;
}
"#;
    assert!(matches!(
        Ir::from_sd(sd_cycle).unwrap_err(),
        Error::InvalidLayout
    ));

    let mut non_prefix = Ir::new(Endianness::Little);
    let (base_ty, _) = prefix_base_derived_layouts();
    let other = StructBuilder::new()
        .field("y", Type::scalar(Scalar::U32))
        .repr_c()
        .unwrap();
    non_prefix.insert("BaseLay", base_ty);
    non_prefix.insert("DerivedLay", other);
    non_prefix
        .insert_class(
            "Base",
            Class {
                layout: "BaseLay".into(),
                base: None,
                methods: Default::default(),
            },
        )
        .unwrap();
    let err = non_prefix
        .insert_class(
            "Derived",
            Class {
                layout: "DerivedLay".into(),
                base: Some("Base".into()),
                methods: Default::default(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::InvalidLayout));

    let mut clash = Ir::new(Endianness::Little);
    let base_ty = StructBuilder::new()
        .field("x", Type::scalar(Scalar::U32))
        .repr_c()
        .unwrap();
    let Type::Struct(base_st) = &base_ty else {
        panic!("base layout must be a struct");
    };
    let mut fields = base_st.fields.clone();
    fields.push(Field {
        name: "x".into(),
        ty: Type::scalar(Scalar::U32),
        offset: 4,
        size: 4,
        align: 4,
    });
    clash.insert("BaseLay", base_ty.clone());
    clash.insert(
        "DerivedLay",
        Type::Struct(StructType {
            fields,
            size: 8,
            align: 4,
        }),
    );
    clash
        .insert_class(
            "Base",
            Class {
                layout: "BaseLay".into(),
                base: None,
                methods: Default::default(),
            },
        )
        .unwrap();
    let err = clash
        .insert_class(
            "Derived",
            Class {
                layout: "DerivedLay".into(),
                base: Some("Base".into()),
                methods: Default::default(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateField));

    let sd_dup_base = r#"endian little
layout DerivedLay repr_c {
  x: u32;
}
class Derived {
  layout DerivedLay;
  base Nope;
  base Nope;
}
"#;
    assert!(matches!(
        Ir::from_sd(sd_dup_base).unwrap_err(),
        Error::Dsl(_)
    ));

    let mut non_struct = Ir::new(Endianness::Little);
    let (base_ty, _) = prefix_base_derived_layouts();
    non_struct.insert("BaseLay", base_ty);
    non_struct.insert("Blob", Type::array(Type::scalar(Scalar::U8), 8).unwrap());
    non_struct
        .insert_class(
            "Base",
            Class {
                layout: "BaseLay".into(),
                base: None,
                methods: Default::default(),
            },
        )
        .unwrap();
    let err = non_struct
        .insert_class(
            "Derived",
            Class {
                layout: "Blob".into(),
                base: Some("Base".into()),
                methods: Default::default(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::InvalidLayout | Error::TypeMismatch));
}
