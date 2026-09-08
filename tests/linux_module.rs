use stardust::{
    CallArg, Endianness, Error, Import, Ir, Library, Param, Path, Region, Ret, Scalar,
    StructBuilder, Type, Value,
};

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

fn device_available() -> bool {
    #[cfg(windows)]
    {
        false
    }
    #[cfg(not(windows))]
    {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/stardust")
            .is_ok()
    }
}

#[test]
fn module_backed() {
    if !device_available() {
        eprintln!("skipping module_backed: /dev/stardust not available");
        return;
    }

    let ir = demo_ir();
    let mut region = Region::map_module("w9", 64, 8).expect("map_module w9");
    assert_eq!(region.name(), "w9");
    let base = region.as_ptr();
    assert!(!base.is_null());

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
        ir.retrieve("Demo", &region, &Path::new().name("xs"))
            .unwrap(),
        Value::Slice {
            offset: payload_offset(&ir),
            elems: vec![Value::U32(10), Value::U32(20)],
        }
    );

    {
        let view = ir.view("Demo", &mut region).unwrap();
        assert_eq!(view.as_ptr(), base);
        assert_eq!(view.get(&Path::new().name("tag")).unwrap(), Value::I32(-7));
    }
    assert_eq!(region.as_ptr(), base);

    let lib = Library::load(stardust_c::process_lib_path()).expect("load W2 C fixture");
    let mut cir = Ir::new(Endianness::Little);
    cir.insert_import(
        "write_a",
        Import::new("write_a", vec![Param::Ptr, Param::Len], Ret::Void),
    )
    .unwrap();
    let write_a = lib.bind(&cir, "write_a").unwrap();
    let before = region.as_bytes().to_vec();
    write_a
        .call(
            &mut region,
            &[
                CallArg::Ptr {
                    region: 0,
                    offset: 8,
                },
                CallArg::Len(16),
            ],
        )
        .unwrap();
    let after = region.as_bytes();
    assert!(
        after[8..24].iter().all(|&b| b == 0xA1),
        "write_a must fill span [8,24) with 0xA1"
    );
    assert_ne!(
        &after[8..24],
        &before[8..24],
        "in-place C must change region bytes"
    );
    assert_eq!(region.as_ptr(), base, "as_ptr() identity must be unchanged");

    let dup = Region::map_module("w9", 64, 8);
    assert!(
        matches!(dup, Err(Error::MapFailed(_))),
        "duplicate CREATE while live must be MapFailed, got {dup:?}"
    );
}
