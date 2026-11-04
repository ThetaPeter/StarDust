use std::collections::BTreeMap;

use stardust::{
    BoundClass, Bump, CallArg, CallRet, CallScalar, Class, Endianness, Error, ExportField, Import,
    Ir, Lang, Library, MethodLoader, Param, Path, Region, Ret, Scalar, StructBuilder, Type, Value,
};

fn load_fixture() -> Library {
    Library::load(stardust_c::process_lib_path()).expect("load C fixture")
}

// I1
fn process_import() -> Import {
    Import::new("process", vec![Param::Ptr, Param::Len], Ret::Void)
}

fn process_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import("process", process_import()).unwrap();
    ir
}

#[test]
fn in_place_c() {
    let ir = process_ir();
    let lib = load_fixture();
    let func = lib.bind(&ir, "process").unwrap();

    let mut region = Region::allocate("w2", 32, 8).unwrap();
    let base = region.as_ptr();
    for (i, b) in region.as_bytes_mut().iter_mut().enumerate() {
        *b = i as u8;
    }
    region.as_bytes_mut()[12] = 255;

    let offset = 8u64;
    let n = 16u64;
    let before = region.as_bytes().to_vec();
    func.call(
        &mut region,
        &[CallArg::Ptr { region: 0, offset }, CallArg::Len(n)],
    )
    .unwrap();

    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    for i in 8..24 {
        assert_eq!(
            after[i],
            before[i].wrapping_add(1),
            "span byte {i} must be +1 wrapping"
        );
    }
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");
    assert_eq!(after[12], 0, "255 + 1 wrapping");
    assert_eq!(
        region.as_ptr(),
        base,
        "region.as_ptr() identity must be unchanged"
    );
}

#[test]
fn reject_incompatible() {
    let lib = load_fixture();

    // malloc-style: callee-owned pointer return is not region-compatible.
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import(
        "process",
        Import::new("process", vec![Param::Ptr, Param::Len], Ret::OwnedPtr),
    )
    .unwrap();
    assert!(matches!(
        lib.bind(&ir, "process"),
        Err(Error::IncompatibleImport)
    ));

    // OwnedPtr must fail before dlsym (the symbol need not exist).
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import(
        "malloc_style",
        Import::new("definitely_not_exported", vec![Param::Ptr], Ret::OwnedPtr),
    )
    .unwrap();
    assert!(matches!(
        lib.bind(&ir, "malloc_style"),
        Err(Error::IncompatibleImport)
    ));
}

mod i1 {
    use super::*;

    #[test]
    fn import_json_round_trip() {
        let ir = process_ir();
        let json = ir.to_json().unwrap();
        let reloaded = Ir::from_json(&json).unwrap();
        assert_eq!(reloaded, ir);

        let documented = r#"{
        "endianness": "Little",
        "layouts": {},
        "imports": {
            "process": {
                "symbol": "process",
                "args": ["Ptr", "Len"],
                "ret": "Void"
            }
        }
    }"#;
        assert_eq!(Ir::from_json(documented).unwrap(), ir);

        let mut with_scalar = Ir::new(Endianness::Little);
        with_scalar
            .insert_import(
                "inc",
                Import::new("inc", vec![Param::Scalar(Scalar::U32)], Ret::Void),
            )
            .unwrap();
        let json = with_scalar.to_json().unwrap();
        assert_eq!(Ir::from_json(&json).unwrap(), with_scalar);
    }
}

#[test]
fn scalar_arg() {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import(
        "add_u32",
        Import::new(
            "add_u32",
            vec![Param::Ptr, Param::Len, Param::Scalar(Scalar::U32)],
            Ret::Void,
        ),
    )
    .unwrap();
    ir.insert_import("process", process_import()).unwrap();

    let lib = load_fixture();
    let add = lib.bind(&ir, "add_u32").unwrap();
    let process = lib.bind(&ir, "process").unwrap();

    let mut region = Region::allocate("o1-arg", 32, 8).unwrap();
    let base = region.as_ptr();
    for (i, b) in region.as_bytes_mut().iter_mut().enumerate() {
        *b = i as u8;
    }
    let before = region.as_bytes().to_vec();
    let offset = 8u64;
    let n = 16u64;

    add.call(
        &mut region,
        &[
            CallArg::Ptr { region: 0, offset },
            CallArg::Len(n),
            CallArg::Scalar(CallScalar::U32(2)),
        ],
    )
    .unwrap();
    assert_eq!(region.as_ptr(), base);

    process
        .call(
            &mut region,
            &[CallArg::Ptr { region: 0, offset }, CallArg::Len(n)],
        )
        .unwrap();
    assert_eq!(region.as_ptr(), base);

    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    for i in 8..24 {
        assert_eq!(
            after[i],
            before[i].wrapping_add(2).wrapping_add(1),
            "span byte {i}"
        );
    }
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");

    let mut bad = Ir::new(Endianness::Little);
    bad.insert_import(
        "owned",
        Import::new(
            "no_such_symbol",
            vec![Param::Scalar(Scalar::U32)],
            Ret::OwnedPtr,
        ),
    )
    .unwrap();
    assert!(matches!(
        lib.bind(&bad, "owned"),
        Err(Error::IncompatibleImport)
    ));
}

#[test]
fn scalar_ret() {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import("write_a", ptr_len_void("write_a"))
        .unwrap();
    ir.insert_import(
        "load_u32",
        Import::new(
            "load_u32",
            vec![Param::Ptr, Param::Len],
            Ret::Scalar(Scalar::U32),
        ),
    )
    .unwrap();

    let lib = load_fixture();
    let write_a = lib.bind(&ir, "write_a").unwrap();
    let load = lib.bind(&ir, "load_u32").unwrap();

    let mut region = Region::allocate("o1-ret", 32, 8).unwrap();
    let base = region.as_ptr();
    fill_non_a1(&mut region);

    write_a
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
    assert_eq!(region.as_ptr(), base);

    let got = load
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
    assert_eq!(region.as_ptr(), base);
    assert_eq!(got, CallRet::Scalar(CallScalar::U32(0xA1A1A1A1)));
}

#[test]
fn duplicate_import() {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import("process", process_import()).unwrap();
    let err = ir.insert_import("process", process_import()).unwrap_err();
    assert!(matches!(err, Error::DuplicateImport));
    assert!(
        !matches!(err, Error::DuplicateField),
        "duplicate import names must not reuse DuplicateField"
    );
}

// I2
fn ptr_len_void(symbol: &str) -> Import {
    Import::new(symbol, vec![Param::Ptr, Param::Len], Ret::Void)
}

fn ab_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import("write_a", ptr_len_void("write_a"))
        .unwrap();
    ir.insert_import("mut_b", ptr_len_void("mut_b")).unwrap();
    ir
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
fn a_then_b() {
    let ir = ab_ir();
    let lib = load_fixture();
    let func_a = lib.bind(&ir, "write_a").unwrap();
    let func_b = lib.bind(&ir, "mut_b").unwrap();

    let mut region = Region::allocate("w3", 32, 8).unwrap();
    fill_non_a1(&mut region);
    let before = region.as_bytes().to_vec();

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

    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    assert!(
        after[SPAN].iter().all(|&b| b == 0xA2),
        "span [8,24) must be 0xA2 after write_a then mut_b, got {:?}",
        &after[SPAN]
    );
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");
}

#[test]
fn same_backing() {
    let ir = ab_ir();
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
        "span [8,24) must be 0xA2 on the same backing, got {:?}",
        &after[SPAN]
    );
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");
}

mod i2 {
    use super::*;

    #[test]
    fn import_json_round_trip() {
        let ir = ab_ir();
        let json = ir.to_json().unwrap();
        let reloaded = Ir::from_json(&json).unwrap();
        assert_eq!(reloaded, ir);
    }
}

fn fill_non_a1_b2(region: &mut Region) {
    for (i, b) in region.as_bytes_mut().iter_mut().enumerate() {
        *b = 0x33u8.wrapping_add(i as u8);
        if *b == 0xA1 || *b == 0xB2 {
            *b = 0x33;
        }
    }
}

fn pair_write_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import(
        "pair_write",
        Import::new(
            "pair_write",
            vec![Param::Ptr, Param::Len, Param::Ptr, Param::Len],
            Ret::Void,
        ),
    )
    .unwrap();
    ir.insert_import("process", process_import()).unwrap();
    ir
}

#[test]
fn two_region_c() {
    let ir = pair_write_ir();
    let lib = load_fixture();
    let pair_write = lib.bind(&ir, "pair_write").unwrap();
    let process = lib.bind(&ir, "process").unwrap();

    let mut a = Region::allocate("i7-a", 32, 8).unwrap();
    let mut b = Region::allocate("i7-b", 32, 8).unwrap();
    fill_non_a1_b2(&mut a);
    fill_non_a1_b2(&mut b);
    let before_a = a.as_bytes().to_vec();
    let before_b = b.as_bytes().to_vec();
    let base_a = a.as_ptr();
    let base_b = b.as_ptr();

    pair_write
        .call_regions(
            &mut [&mut a, &mut b],
            &[
                CallArg::Ptr {
                    region: 0,
                    offset: OFFSET,
                },
                CallArg::Len(LEN),
                CallArg::Ptr {
                    region: 1,
                    offset: OFFSET,
                },
                CallArg::Len(LEN),
            ],
        )
        .unwrap();

    assert_eq!(a.as_ptr(), base_a, "as_ptr() identity on region A");
    assert_eq!(b.as_ptr(), base_b, "as_ptr() identity on region B");

    let after_a = a.as_bytes();
    let after_b = b.as_bytes();
    assert_eq!(&after_a[..8], &before_a[..8], "region A prefix unchanged");
    assert!(
        after_a[SPAN].iter().all(|&byte| byte == 0xA1),
        "region A span [8,24) must be 0xA1, got {:?}",
        &after_a[SPAN]
    );
    assert_eq!(&after_a[24..], &before_a[24..], "region A suffix unchanged");
    assert_eq!(&after_b[..8], &before_b[..8], "region B prefix unchanged");
    assert!(
        after_b[SPAN].iter().all(|&byte| byte == 0xB2),
        "region B span [8,24) must be 0xB2, got {:?}",
        &after_b[SPAN]
    );
    assert_eq!(&after_b[24..], &before_b[24..], "region B suffix unchanged");

    assert!(
        matches!(
            pair_write.call(
                &mut a,
                &[
                    CallArg::Ptr {
                        region: 1,
                        offset: OFFSET
                    },
                    CallArg::Len(LEN),
                    CallArg::Ptr {
                        region: 0,
                        offset: OFFSET
                    },
                    CallArg::Len(LEN),
                ],
            ),
            Err(Error::TypeMismatch)
        ),
        "call with Ptr.region != 0 must be TypeMismatch"
    );

    let mut region = Region::allocate("i7-process", 32, 8).unwrap();
    let base = region.as_ptr();
    for (i, byte) in region.as_bytes_mut().iter_mut().enumerate() {
        *byte = i as u8;
    }
    region.as_bytes_mut()[12] = 255;
    let before = region.as_bytes().to_vec();
    process
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
    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "process prefix unchanged");
    for i in 8..24 {
        assert_eq!(
            after[i],
            before[i].wrapping_add(1),
            "process span byte {i} must be +1 wrapping"
        );
    }
    assert_eq!(&after[24..], &before[24..], "process suffix unchanged");
    assert_eq!(after[12], 0, "255 + 1 wrapping");
    assert_eq!(region.as_ptr(), base, "process as_ptr() identity");

    assert!(
        matches!(
            pair_write.call_regions(
                &mut [&mut a, &mut b],
                &[
                    CallArg::Ptr {
                        region: 0,
                        offset: OFFSET
                    },
                    CallArg::Len(LEN),
                    CallArg::Ptr {
                        region: 2,
                        offset: OFFSET
                    },
                    CallArg::Len(LEN),
                ],
            ),
            Err(Error::OutOfBounds)
        ),
        "call_regions region index >= len must be OutOfBounds"
    );
}

fn class_c_ir() -> Ir {
    let blob = Type::array(Type::scalar(Scalar::U8), 16).unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Blob", blob);
    let mut widget = Class {
        layout: "Blob".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    widget
        .insert_method(
            "write_a",
            Import::new("write_a", vec![Param::Ptr, Param::Len], Ret::Void).with_lang(Lang::C),
        )
        .unwrap();
    ir.insert_class("Widget", widget).unwrap();
    ir
}

#[test]
fn class_c_method() {
    let ir = class_c_ir();
    assert!(
        ir.imports.is_empty(),
        "class methods live on the class, not Ir.imports"
    );
    let lib = load_fixture();
    let mut loaders = BTreeMap::new();
    loaders.insert("write_a".into(), MethodLoader::Library(&lib));
    let bound = BoundClass::bind(&ir, "Widget", &loaders).unwrap();

    let mut region = Region::allocate("o3", 32, 8).unwrap();
    fill_non_a1(&mut region);
    let before = region.as_bytes().to_vec();
    let base = region.as_ptr();

    let ret = bound.call("write_a", &mut region, OFFSET).unwrap();
    assert_eq!(ret, CallRet::Void);
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through class write_a"
    );

    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    assert!(
        after[SPAN].iter().all(|&b| b == 0xA1),
        "span [8,24) must be 0xA1 after class write_a, got {:?}",
        &after[SPAN]
    );
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");

    assert!(matches!(
        BoundClass::bind(&ir, "Nope", &loaders),
        Err(Error::UnknownClass)
    ));
    let empty = BTreeMap::new();
    assert!(matches!(
        BoundClass::bind(&ir, "Widget", &empty),
        Err(Error::MissingLoader(name)) if name == "write_a"
    ));
    assert!(matches!(
        bound.call("nope", &mut region, OFFSET),
        Err(Error::UnknownMethod)
    ));
}

#[test]
fn class_bump_place() {
    let ir = class_c_ir();
    let lib = load_fixture();
    let mut loaders = BTreeMap::new();
    loaders.insert("write_a".into(), MethodLoader::Library(&lib));
    let bound = BoundClass::bind(&ir, "Widget", &loaders).unwrap();

    let mut region = Region::allocate("o5", 64, 8).unwrap();
    fill_non_a1(&mut region);
    let before = region.as_bytes().to_vec();
    let base = region.as_ptr();

    let (a, b) = {
        let mut bump = Bump::new(&mut region);
        let a = bound.place(&mut bump).unwrap();
        let b = bound.place(&mut bump).unwrap();
        assert_eq!(a, 8, "first bump slot starts at offset 8");
        assert_eq!(b, 24, "second bump slot follows 16-byte Blob");
        assert!(b >= a + 16, "instance spans must not overlap");
        (a, b)
    };

    bound.call("write_a", &mut region, a).unwrap();
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through write_a on first instance"
    );
    bound.call("write_a", &mut region, b).unwrap();
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through write_a on second instance"
    );

    let after = region.as_bytes();
    let a_start = usize::try_from(a).unwrap();
    let b_start = usize::try_from(b).unwrap();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    assert!(
        after[a_start..a_start + 16]
            .iter()
            .all(|&byte| byte == 0xA1),
        "first instance span must be 0xA1, got {:?}",
        &after[a_start..a_start + 16]
    );
    assert!(
        after[b_start..b_start + 16]
            .iter()
            .all(|&byte| byte == 0xA1),
        "second instance span must be 0xA1, got {:?}",
        &after[b_start..b_start + 16]
    );
    assert_eq!(
        &after[a_start + 16..b_start],
        &before[a_start + 16..b_start],
        "gap between instances must be unchanged"
    );
    assert_eq!(
        &after[b_start + 16..],
        &before[b_start + 16..],
        "suffix must be unchanged"
    );
}

fn prefix_override_ir() -> Ir {
    let base_ty = StructBuilder::new()
        .field("x", Type::scalar(Scalar::U32))
        .repr_c()
        .unwrap();
    let derived_ty = StructBuilder::new()
        .field("x", Type::scalar(Scalar::U32))
        .field("y", Type::scalar(Scalar::U32))
        .repr_c()
        .unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("BaseLay", base_ty);
    ir.insert("DerivedLay", derived_ty);
    let mut base = Class {
        layout: "BaseLay".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
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
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
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
fn prefix_override() {
    let ir = prefix_override_ir();
    assert!(
        ir.imports.is_empty(),
        "class methods live on the class, not Ir.imports"
    );
    let lib = load_fixture();
    let mut loaders = BTreeMap::new();
    loaders.insert("paint".into(), MethodLoader::Library(&lib));

    let bound_derived = BoundClass::bind(&ir, "Derived", &loaders).unwrap();
    let mut region = Region::allocate("o4", 32, 8).unwrap();
    fill_non_a1(&mut region);
    let before = region.as_bytes().to_vec();
    let base_ptr = region.as_ptr();

    let ret = bound_derived.call("paint", &mut region, OFFSET).unwrap();
    assert_eq!(ret, CallRet::Void);
    assert_eq!(
        region.as_ptr(),
        base_ptr,
        "as_ptr() must be unchanged through derived paint"
    );

    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    for i in 8..16 {
        assert_eq!(
            after[i],
            before[i].wrapping_add(1),
            "derived paint must run mut_b on declared size 8, byte {i}"
        );
    }
    assert_eq!(&after[16..], &before[16..], "suffix must be unchanged");

    fill_non_a1(&mut region);
    let before = region.as_bytes().to_vec();
    let bound_base = BoundClass::bind(&ir, "Base", &loaders).unwrap();
    let ret = bound_base.call("paint", &mut region, OFFSET).unwrap();
    assert_eq!(ret, CallRet::Void);
    assert_eq!(
        region.as_ptr(),
        base_ptr,
        "as_ptr() must be unchanged through base paint"
    );

    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    assert!(
        after[8..12].iter().all(|&b| b == 0xA1),
        "base paint must run write_a on declared size 4, got {:?}",
        &after[8..12]
    );
    assert_eq!(
        &after[12..16],
        &before[12..16],
        "derived suffix y must be unchanged under a Base view"
    );
    assert_eq!(&after[16..], &before[16..], "suffix must be unchanged");
}

fn class_plus_u64_ir() -> Ir {
    let blob = Type::array(Type::scalar(Scalar::U8), 16).unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Blob", blob);
    let mut widget = Class {
        layout: "Blob".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    widget
        .insert_method(
            "plus",
            Import::new(
                "plus_u64",
                vec![Param::Ptr, Param::Len, Param::Scalar(Scalar::U64)],
                Ret::Scalar(Scalar::U64),
            )
            .with_lang(Lang::C),
        )
        .unwrap();
    ir.insert_class("Widget", widget).unwrap();
    ir
}

fn class_stamp_out_ir() -> Ir {
    let blob = Type::array(Type::scalar(Scalar::U8), 16).unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Blob", blob);
    let mut widget = Class {
        layout: "Blob".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    widget
        .insert_method(
            "stamp",
            Import::new(
                "stamp_out",
                vec![Param::Ptr, Param::Len, Param::Ptr],
                Ret::Void,
            )
            .with_lang(Lang::C),
        )
        .unwrap();
    ir.insert_class("Widget", widget).unwrap();
    ir
}

#[test]
fn class_extra_scalar() {
    let ir = class_plus_u64_ir();
    let lib = load_fixture();
    let mut loaders = BTreeMap::new();
    loaders.insert("plus".into(), MethodLoader::Library(&lib));
    let bound = BoundClass::bind(&ir, "Widget", &loaders).unwrap();

    let mut region = Region::allocate("o6-scalar", 32, 8).unwrap();
    fill_non_a1(&mut region);
    region.as_bytes_mut()[8..16].copy_from_slice(&10u64.to_le_bytes());
    let base = region.as_ptr();
    let before = region.as_bytes().to_vec();

    let ret = bound
        .call_with(
            "plus",
            &mut region,
            OFFSET,
            &[CallArg::Scalar(CallScalar::U64(5))],
        )
        .unwrap();
    assert_eq!(ret, CallRet::Scalar(CallScalar::U64(15)));
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through class plus_u64"
    );
    assert_eq!(
        region.as_bytes(),
        before.as_slice(),
        "plus_u64 must not write the instance span"
    );
}

#[test]
fn class_out_ptr() {
    let ir = class_stamp_out_ir();
    let lib = load_fixture();
    let mut loaders = BTreeMap::new();
    loaders.insert("stamp".into(), MethodLoader::Library(&lib));
    let bound = BoundClass::bind(&ir, "Widget", &loaders).unwrap();

    let mut region = Region::allocate("o6-out", 64, 8).unwrap();
    fill_non_a1(&mut region);
    let base = region.as_ptr();
    let before = region.as_bytes().to_vec();
    let out_off = 40u64;

    let ret = bound
        .call_with(
            "stamp",
            &mut region,
            OFFSET,
            &[CallArg::Ptr {
                region: 0,
                offset: out_off,
            }],
        )
        .unwrap();
    assert_eq!(ret, CallRet::Void);
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through class stamp_out"
    );

    let after = region.as_bytes();
    let out = usize::try_from(out_off).unwrap();
    assert_eq!(
        &after[..out],
        &before[..out],
        "bytes before out must be unchanged"
    );
    assert!(
        after[out..out + 8].iter().all(|&b| b == 0xC3),
        "out [40,48) must be 0xC3, got {:?}",
        &after[out..out + 8]
    );
    assert_eq!(
        &after[out + 8..],
        &before[out + 8..],
        "bytes after out must be unchanged"
    );
}

#[test]
fn class_reject_foreign_ptr() {
    let ir = class_stamp_out_ir();
    let lib = load_fixture();
    let mut loaders = BTreeMap::new();
    loaders.insert("stamp".into(), MethodLoader::Library(&lib));
    let bound = BoundClass::bind(&ir, "Widget", &loaders).unwrap();

    let mut region = Region::allocate("o6-reject", 64, 8).unwrap();
    fill_non_a1(&mut region);
    let before = region.as_bytes().to_vec();
    let base = region.as_ptr();

    assert!(matches!(
        bound.call_with(
            "stamp",
            &mut region,
            OFFSET,
            &[CallArg::Ptr {
                region: 1,
                offset: 0
            }],
        ),
        Err(Error::TypeMismatch)
    ));
    assert_eq!(region.as_ptr(), base);
    assert_eq!(
        region.as_bytes(),
        before.as_slice(),
        "reject must not call stamp_out"
    );

    assert!(matches!(
        bound.call_with("stamp", &mut region, OFFSET, &[CallArg::Len(8)]),
        Err(Error::UnsupportedCall)
    ));
}

fn two_ptr_len() -> Vec<Param> {
    vec![Param::Ptr, Param::Len]
}

fn four_ptr_len() -> Vec<Param> {
    vec![Param::Ptr, Param::Len, Param::Ptr, Param::Len]
}

fn export_field_import(symbol: &str) -> Import {
    Import::new(symbol, four_ptr_len(), Ret::Void).with_lang(Lang::Rust)
}

fn export_method_import(symbol: &str) -> Import {
    Import::new(symbol, two_ptr_len(), Ret::Void).with_lang(Lang::Rust)
}

fn o8_export_ir() -> Ir {
    let student_ty = StructBuilder::new()
        .field("major", Type::scalar(Scalar::U32))
        .field("db", Type::link())
        .field("activity", Type::link())
        .repr_c()
        .unwrap();
    let db_ty = StructBuilder::new()
        .field("len", Type::scalar(Scalar::U64))
        .field("buf", Type::array(Type::scalar(Scalar::U8), 32).unwrap())
        .repr_c()
        .unwrap();
    let minor_ty = StructBuilder::new()
        .field("data", Type::array(Type::scalar(Scalar::U8), 8).unwrap())
        .repr_c()
        .unwrap();
    let act_ty = StructBuilder::new()
        .field("tag", Type::scalar(Scalar::U8))
        .repr_c()
        .unwrap();
    assert_eq!(student_ty.inline_size(), 24);
    assert_eq!(db_ty.inline_size(), 40);
    assert_eq!(minor_ty.inline_size(), 8);

    let mut ir = Ir::new(Endianness::Little);
    ir.insert("StudentLay", student_ty);
    ir.insert("DbLay", db_ty);
    ir.insert("MinorLay", minor_ty);
    ir.insert("ActLay", act_ty);

    let mut db = Class {
        layout: "DbLay".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    db.insert_export_field(
        "minor",
        ExportField {
            layout: "MinorLay".into(),
            assign: export_field_import("set_minor"),
            retrieve: export_field_import("get_minor"),
        },
    )
    .unwrap();
    db.insert_export_method("print_minor", export_method_import("print_minor"))
        .unwrap();
    ir.insert_class("StudentMinorDatabase", db).unwrap();

    ir.insert_class(
        "Student",
        Class {
            layout: "StudentLay".into(),
            base: None,
            methods: Default::default(),
            local_fields: Default::default(),
            export_fields: Default::default(),
            export_methods: Default::default(),
        },
    )
    .unwrap();

    let mut activity = Class {
        layout: "ActLay".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    activity
        .insert_export_field(
            "minor",
            ExportField {
                layout: "MinorLay".into(),
                assign: export_field_import("set_minor"),
                retrieve: export_field_import("get_minor"),
            },
        )
        .unwrap();
    ir.insert_class("ActivityDb", activity).unwrap();
    ir
}

#[test]
fn rust_lang_ir() {
    let lay = StructBuilder::new()
        .field("b", Type::scalar(Scalar::U8))
        .repr_c()
        .unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("TouchLay", lay);
    let mut class = Class {
        layout: "TouchLay".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    class
        .insert_method(
            "touch",
            Import::new("touch", vec![Param::Ptr, Param::Len], Ret::Void).with_lang(Lang::Rust),
        )
        .unwrap();
    ir.insert_class("Touch", class).unwrap();

    let json = ir.to_json().unwrap();
    assert!(json.contains("Rust"), "JSON must contain Rust, got {json}");
    assert_eq!(Ir::from_json(&json).unwrap(), ir);

    let sd = ir.to_sd().expect("to_sd");
    assert!(
        sd.contains("lang rust"),
        "sd must contain lang rust, got {sd}"
    );
    assert_eq!(Ir::from_sd(&sd).unwrap(), ir);

    let lib = Library::load(stardust_rs::project_lib_path()).expect("load rust fixture");
    let mut loaders = BTreeMap::new();
    loaders.insert("touch".into(), MethodLoader::Library(&lib));
    let bound = BoundClass::bind(&ir, "Touch", &loaders).unwrap();

    let mut region = Region::allocate("o7-touch", 16, 8).unwrap();
    let ret = bound.call("touch", &mut region, 0).unwrap();
    assert_eq!(ret, CallRet::Void);
    assert_eq!(region.as_bytes()[0], 1);

    #[cfg(feature = "python")]
    {
        let mut py_loaders = BTreeMap::new();
        py_loaders.insert(
            "touch".into(),
            MethodLoader::Python {
                script: std::path::PathBuf::from("nope.py"),
            },
        );
        assert!(matches!(
            BoundClass::bind(&ir, "Touch", &py_loaders),
            Err(Error::TypeMismatch)
        ));
    }
}

#[test]
fn export_link_call() {
    let ir = o8_export_ir();
    let lib = Library::load(stardust_rs::project_lib_path()).expect("load rust fixture");
    let mut db_loaders = BTreeMap::new();
    db_loaders.insert("minor".into(), MethodLoader::Library(&lib));
    db_loaders.insert("print_minor".into(), MethodLoader::Library(&lib));
    let db_bound = BoundClass::bind(&ir, "StudentMinorDatabase", &db_loaders).unwrap();
    let student_bound = BoundClass::bind(&ir, "Student", &BTreeMap::new()).unwrap();
    let mut act_loaders = BTreeMap::new();
    act_loaders.insert("minor".into(), MethodLoader::Library(&lib));
    let act_bound = BoundClass::bind(&ir, "ActivityDb", &act_loaders).unwrap();

    const STUDENT: u64 = 0;
    const DB: u64 = 32;
    const ACT: u64 = 80;
    const VALUE: u64 = 96;
    const OUT: u64 = 112;
    const MAJOR: u32 = 0xAABB_CCDD;
    let mut region = Region::allocate("o8-export", 160, 8).unwrap();
    for b in region.as_bytes_mut() {
        *b = 0xA5;
    }
    region.as_bytes_mut()[32..72].fill(0);
    ir.store(
        "StudentLay",
        &mut region,
        &Path::new().name("major"),
        &Value::U32(MAJOR),
    )
    .unwrap();
    ir.store(
        "StudentLay",
        &mut region,
        &Path::new().name("db"),
        &Value::U64(0),
    )
    .unwrap();
    ir.store(
        "StudentLay",
        &mut region,
        &Path::new().name("activity"),
        &Value::U64(0),
    )
    .unwrap();
    region.as_bytes_mut()[96..104].copy_from_slice(b"abcdefgh");

    let mut student = student_bound.handle(STUDENT);
    let dbh = db_bound.handle(DB);
    let acth = act_bound.handle(ACT);

    assert!(matches!(
        student.field_assign("minor", &mut region, VALUE, 8),
        Err(Error::UnknownExport)
    ));
    assert!(matches!(
        db_bound.call("print_minor", &mut region, DB),
        Err(Error::UnknownMethod)
    ));
    assert!(matches!(
        db_bound.call("minor", &mut region, DB),
        Err(Error::UnknownMethod)
    ));
    assert!(matches!(
        dbh.field_assign("minor", &mut region, VALUE, 8),
        Err(Error::UnknownExport)
    ));
    assert!(matches!(
        dbh.call_export("print_minor", &mut region),
        Err(Error::UnknownExport)
    ));

    let base = region.as_ptr();
    student.link("db", &dbh, &mut region).unwrap();
    assert_eq!(
        ir.retrieve("StudentLay", &region, &Path::new().name("db"))
            .unwrap(),
        Value::U64(DB)
    );
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through link"
    );

    student
        .field_assign("minor", &mut region, VALUE, 8)
        .unwrap();
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through assign"
    );
    student
        .field_retrieve("minor", &mut region, OUT, 8)
        .unwrap();
    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() must be unchanged through retrieve"
    );
    assert_eq!(&region.as_bytes()[112..120], b"abcdefgh");
    assert_eq!(
        ir.retrieve_at("DbLay", &region, DB, &Path::new().name("len"))
            .unwrap(),
        Value::U64(8)
    );
    assert_eq!(
        ir.retrieve("StudentLay", &region, &Path::new().name("major"))
            .unwrap(),
        Value::U32(MAJOR)
    );

    student.call_export("print_minor", &mut region).unwrap();
    assert_eq!(region.as_bytes()[71], 1, "print_minor marks last db byte");

    ir.store(
        "StudentLay",
        &mut region,
        &Path::new().name("db"),
        &Value::U64(0),
    )
    .unwrap();
    let before = region.as_bytes().to_vec();
    assert!(matches!(
        student.field_assign("minor", &mut region, VALUE, 8),
        Err(Error::Unlinked)
    ));
    assert_eq!(
        region.as_bytes(),
        before.as_slice(),
        "Unlinked must not write"
    );

    ir.store(
        "StudentLay",
        &mut region,
        &Path::new().name("db"),
        &Value::U64(DB),
    )
    .unwrap();
    assert!(matches!(
        student.link("activity", &acth, &mut region),
        Err(Error::DuplicateExport)
    ));

    let mut bad_sig = ir.clone();
    bad_sig
        .classes
        .get_mut("StudentMinorDatabase")
        .unwrap()
        .export_fields
        .get_mut("minor")
        .unwrap()
        .assign
        .args = vec![Param::Ptr, Param::Len];
    assert!(matches!(
        BoundClass::bind(&bad_sig, "StudentMinorDatabase", &db_loaders),
        Err(Error::UnsupportedCall)
    ));

    let mut tail_ir = Ir::new(Endianness::Little);
    tail_ir.insert("Blob", Type::array(Type::scalar(Scalar::U8), 8).unwrap());
    let mut tail = Class {
        layout: "Blob".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    tail.insert_method(
        "tail",
        Import::new(
            "definitely_not_exported",
            vec![Param::Ptr, Param::Len, Param::Ptr, Param::Len],
            Ret::Void,
        )
        .with_lang(Lang::Rust),
    )
    .unwrap();
    tail_ir.insert_class("Tail", tail).unwrap();
    let mut tail_loaders = BTreeMap::new();
    tail_loaders.insert("tail".into(), MethodLoader::Library(&lib));
    assert!(matches!(
        BoundClass::bind(&tail_ir, "Tail", &tail_loaders),
        Err(Error::UnsupportedCall)
    ));

    #[cfg(feature = "python")]
    {
        let mut py_loaders = BTreeMap::new();
        py_loaders.insert(
            "minor".into(),
            MethodLoader::Python {
                script: std::path::PathBuf::from("nope.py"),
            },
        );
        py_loaders.insert("print_minor".into(), MethodLoader::Library(&lib));
        assert!(matches!(
            BoundClass::bind(&ir, "StudentMinorDatabase", &py_loaders),
            Err(Error::TypeMismatch)
        ));
    }
}

fn local_assign_ir() -> Ir {
    let db_ty = StructBuilder::new()
        .field("len", Type::scalar(Scalar::U64))
        .field("buf", Type::array(Type::scalar(Scalar::U8), 32).unwrap())
        .repr_c()
        .unwrap();
    let big_ty = StructBuilder::new()
        .field("len", Type::scalar(Scalar::U64))
        .field("buf", Type::array(Type::scalar(Scalar::U8), 32).unwrap())
        .field("extra", Type::scalar(Scalar::U64))
        .repr_c()
        .unwrap();
    let minor_ty = StructBuilder::new()
        .field("data", Type::array(Type::scalar(Scalar::U8), 8).unwrap())
        .repr_c()
        .unwrap();
    let student_ty = StructBuilder::new()
        .field("major", Type::scalar(Scalar::U32))
        .field("db", Type::link())
        .repr_c()
        .unwrap();
    assert_eq!(db_ty.inline_size(), 40);
    assert_eq!(big_ty.inline_size(), 48);
    assert_eq!(minor_ty.inline_size(), 8);

    let mut ir = Ir::new(Endianness::Little);
    ir.insert("DbLay", db_ty);
    ir.insert("BigDbLay", big_ty);
    ir.insert("MinorLay", minor_ty);
    ir.insert("StudentLay", student_ty);

    let minor_field = ExportField {
        layout: "MinorLay".into(),
        assign: export_field_import("set_minor"),
        retrieve: export_field_import("get_minor"),
    };
    let seen_field = ExportField {
        layout: "MinorLay".into(),
        assign: export_field_import("record_span"),
        retrieve: export_field_import("record_span"),
    };
    let mark_field = ExportField {
        layout: "MinorLay".into(),
        assign: Import::new("pair_write", four_ptr_len(), Ret::Void).with_lang(Lang::C),
        retrieve: Import::new("pair_write", four_ptr_len(), Ret::Void).with_lang(Lang::C),
    };

    let mut holder = Class {
        layout: "DbLay".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    holder
        .insert_local_field("minor", minor_field.clone())
        .unwrap();
    holder.insert_local_field("seen", seen_field).unwrap();
    holder.insert_local_field("mark", mark_field).unwrap();
    ir.insert_class("Holder", holder).unwrap();

    ir.insert_class(
        "BigHolder",
        Class {
            layout: "BigDbLay".into(),
            base: Some("Holder".into()),
            methods: Default::default(),
            local_fields: Default::default(),
            export_fields: Default::default(),
            export_methods: Default::default(),
        },
    )
    .unwrap();

    let mut client = Class {
        layout: "StudentLay".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    client
        .insert_local_field("minor", minor_field.clone())
        .unwrap();
    ir.insert_class("Client", client).unwrap();

    let mut exporter = Class {
        layout: "DbLay".into(),
        base: None,
        methods: Default::default(),
        local_fields: Default::default(),
        export_fields: Default::default(),
        export_methods: Default::default(),
    };
    exporter.insert_export_field("minor", minor_field).unwrap();
    ir.insert_class("Exporter", exporter).unwrap();
    ir
}

#[test]
fn local_field_call() {
    let ir = local_assign_ir();
    let rust_lib = Library::load(stardust_rs::project_lib_path()).expect("load rust fixture");
    let c_lib = load_fixture();
    let mut loaders = BTreeMap::new();
    loaders.insert("minor".into(), MethodLoader::Library(&rust_lib));
    loaders.insert("seen".into(), MethodLoader::Library(&rust_lib));
    // Second local field uses the C fixture (`pair_write`) so bind is two real libraries.
    loaders.insert("mark".into(), MethodLoader::Library(&c_lib));
    let holder = BoundClass::bind(&ir, "Holder", &loaders).unwrap();
    let big = BoundClass::bind(&ir, "BigHolder", &loaders).unwrap();

    let mut export_loaders = BTreeMap::new();
    export_loaders.insert("minor".into(), MethodLoader::Library(&rust_lib));
    let client_bound = BoundClass::bind(&ir, "Client", &export_loaders).unwrap();
    let exporter_bound = BoundClass::bind(&ir, "Exporter", &export_loaders).unwrap();

    const HOLDER: u64 = 0;
    const BIG: u64 = 64;
    const CLIENT: u64 = 128;
    const EXPORTER: u64 = 160;
    const VALUE: u64 = 224;
    const OUT: u64 = 240;
    let mut region = Region::allocate("local-field", 288, 8).unwrap();
    for b in region.as_bytes_mut() {
        *b = 0xA5;
    }
    region.as_bytes_mut()[224..232].copy_from_slice(b"abcdefgh");

    let h = holder.handle(HOLDER);
    assert!(matches!(
        holder.call("minor", &mut region, HOLDER),
        Err(Error::UnknownMethod)
    ));
    h.field_assign("minor", &mut region, VALUE, 8).unwrap();
    h.field_retrieve("minor", &mut region, OUT, 8).unwrap();
    assert_eq!(&region.as_bytes()[240..248], b"abcdefgh");
    assert!(matches!(
        h.field_assign("minor", &mut region, VALUE, 7),
        Err(Error::TypeMismatch)
    ));

    h.field_assign("seen", &mut region, VALUE, 8).unwrap();
    let mut span = [0u8; 8];
    span.copy_from_slice(&region.as_bytes()[0..8]);
    assert_eq!(
        u64::from_le_bytes(span),
        40,
        "base handle must pass base inline_size"
    );

    let bh = big.handle(BIG);
    bh.field_assign("seen", &mut region, VALUE, 8).unwrap();
    span.copy_from_slice(&region.as_bytes()[64..72]);
    assert_eq!(
        u64::from_le_bytes(span),
        48,
        "derived handle must pass derived inline_size"
    );

    region.as_bytes_mut()[64..112].fill(0);
    bh.field_assign("minor", &mut region, VALUE, 8).unwrap();
    bh.field_retrieve("minor", &mut region, OUT, 8).unwrap();
    assert_eq!(&region.as_bytes()[240..248], b"abcdefgh");

    h.field_assign("mark", &mut region, VALUE, 8).unwrap();
    assert!(
        region.as_bytes()[0..40].iter().all(|&b| b == 0xA1),
        "pair_write fills the base instance span"
    );

    let mut client = client_bound.handle(CLIENT);
    let exporter = exporter_bound.handle(EXPORTER);
    assert!(matches!(
        client.link("db", &exporter, &mut region),
        Err(Error::DuplicateExport)
    ));
}
