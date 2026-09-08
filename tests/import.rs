use std::collections::BTreeMap;

use stardust::{
    BoundClass, Bump, CallArg, CallRet, CallScalar, Class, Endianness, Error, Import, Ir, Lang,
    Library, MethodLoader, Param, Region, Ret, Scalar, StructBuilder, Type,
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
