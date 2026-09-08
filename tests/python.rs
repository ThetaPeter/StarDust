use std::collections::BTreeMap;
use std::path::PathBuf;

use stardust::{
    call_python, BoundClass, CallArg, CallRet, Class, Endianness, Error, Import, Ir, Lang, Library,
    MethodLoader, Param, Region, Ret, Scalar, Type,
};

fn ptr_len_void(symbol: &str) -> Import {
    Import::new(symbol, vec![Param::Ptr, Param::Len], Ret::Void)
}

fn write_a_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import("write_a", ptr_len_void("write_a"))
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

fn mutate_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/w7_mutate.py")
}

const OFFSET: u64 = 8;
const LEN: u64 = 16;
const SPAN: std::ops::Range<usize> = 8..24;

#[test]
fn shared_region() {
    let ir = write_a_ir();
    let lib = load_fixture();
    let func_a = lib.bind(&ir, "write_a").unwrap();

    let mut region = Region::allocate("w7", 32, 8).unwrap();
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
    assert_eq!(
        before_a, after_a,
        "as_ptr() must be unchanged through write_a"
    );
    assert!(
        region.as_bytes()[SPAN].iter().all(|&b| b == 0xA1),
        "span [8,24) must be 0xA1 after write_a, got {:?}",
        &region.as_bytes()[SPAN]
    );

    call_python(&mut region, OFFSET, LEN, mutate_script(), "mutate").unwrap();
    let after_py = region.as_ptr();
    assert_eq!(
        after_a, after_py,
        "as_ptr() must be unchanged through Python mutate"
    );
    assert_eq!(
        before_a, after_py,
        "C and Python must share the same region base"
    );

    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    assert!(
        after[SPAN].iter().all(|&b| b == 0xA2),
        "span [8,24) must be 0xA2 after write_a then Python +1, got {:?}",
        &after[SPAN]
    );
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");
}

fn class_multi_ir() -> Ir {
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
    widget
        .insert_method(
            "mutate",
            Import::new("mutate", vec![Param::Ptr, Param::Len], Ret::Void).with_lang(Lang::Python),
        )
        .unwrap();
    ir.insert_class("Widget", widget).unwrap();
    ir
}

#[test]
fn class_multi_lang() {
    let ir = class_multi_ir();
    assert!(
        ir.imports.is_empty(),
        "class methods live on the class, not Ir.imports"
    );
    let lib = load_fixture();
    let mut loaders = BTreeMap::new();
    loaders.insert("write_a".into(), MethodLoader::Library(&lib));
    loaders.insert(
        "mutate".into(),
        MethodLoader::Python {
            script: mutate_script(),
        },
    );
    let bound = BoundClass::bind(&ir, "Widget", &loaders).unwrap();

    let mut region = Region::allocate("o3", 32, 8).unwrap();
    fill_non_a1(&mut region);
    let before = region.as_bytes().to_vec();
    let base = region.as_ptr();

    let ret_a = bound.call("write_a", &mut region, OFFSET).unwrap();
    assert_eq!(ret_a, CallRet::Void);
    let after_a = region.as_ptr();
    assert_eq!(
        base, after_a,
        "as_ptr() must be unchanged through class write_a"
    );

    let ret_py = bound.call("mutate", &mut region, OFFSET).unwrap();
    assert_eq!(ret_py, CallRet::Void);
    let after_py = region.as_ptr();
    assert_eq!(
        after_a, after_py,
        "as_ptr() must be unchanged through class mutate"
    );
    assert_eq!(
        base, after_py,
        "C and Python must share the same region base"
    );

    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    assert!(
        after[SPAN].iter().all(|&b| b == 0xA2),
        "span [8,24) must be 0xA2 after class write_a then mutate, got {:?}",
        &after[SPAN]
    );
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");

    let mut mismatch = BTreeMap::new();
    mismatch.insert(
        "write_a".into(),
        MethodLoader::Python {
            script: mutate_script(),
        },
    );
    mismatch.insert("mutate".into(), MethodLoader::Library(&lib));
    assert!(matches!(
        BoundClass::bind(&ir, "Widget", &mismatch),
        Err(Error::TypeMismatch)
    ));
}
