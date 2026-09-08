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

fn assert_demo(ir: &Ir, region: &Region) {
    assert_eq!(
        ir.retrieve("Demo", region, &Path::new().name("tag"))
            .unwrap(),
        Value::I32(-7)
    );
    assert_eq!(
        ir.retrieve("Demo", region, &Path::new().name("ratio"))
            .unwrap(),
        Value::F64(1.25)
    );
    assert_eq!(
        ir.retrieve("Demo", region, &Path::new().name("arr"))
            .unwrap(),
        Value::Array(vec![Value::U16(1), Value::U16(2), Value::U16(3)])
    );
    assert_eq!(
        ir.retrieve("Demo", region, &Path::new().name("xs"))
            .unwrap(),
        Value::Slice {
            offset: payload_offset(ir),
            elems: vec![Value::U32(10), Value::U32(20)],
        }
    );
}

fn device_available() -> bool {
    #[cfg(windows)]
    {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(r"\\.\stardust")
            .is_ok()
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/stardust")
            .is_ok()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

fn write_a_fill(region: &mut Region) {
    let lib = Library::load(stardust_c::process_lib_path()).expect("load W2 C fixture");
    let mut cir = Ir::new(Endianness::Little);
    cir.insert_import(
        "write_a",
        Import::new("write_a", vec![Param::Ptr, Param::Len], Ret::Void),
    )
    .unwrap();
    let write_a = lib.bind(&cir, "write_a").unwrap();
    write_a
        .call(
            region,
            &[
                CallArg::Ptr {
                    region: 0,
                    offset: 8,
                },
                CallArg::Len(16),
            ],
        )
        .unwrap();
}

fn child_attach() {
    let ptr_a: usize = std::env::var("STARDUST_W16_PTR")
        .expect("STARDUST_W16_PTR")
        .parse()
        .expect("STARDUST_W16_PTR usize");

    let ir = demo_ir();
    let mut region = Region::attach_module("w16").expect("attach_module w16");
    assert_eq!(region.name(), "w16");
    assert_eq!(region.size(), 64);
    assert_eq!(region.align(), 8);
    assert_demo(&ir, &region);

    let base = region.as_ptr();
    assert!(!base.is_null());
    assert_ne!(
        base as usize, ptr_a,
        "child mapping VA must differ from parent"
    );

    {
        let view = ir.view("Demo", &mut region).unwrap();
        assert_eq!(view.as_ptr(), base);
        assert_eq!(view.get(&Path::new().name("tag")).unwrap(), Value::I32(-7));
    }
    assert_eq!(region.as_ptr(), base);

    write_a_fill(&mut region);
    assert!(
        region.as_bytes()[8..24].iter().all(|&b| b == 0xA1),
        "write_a must fill span [8,24) with 0xA1"
    );
    assert_eq!(region.as_ptr(), base, "as_ptr() identity must be unchanged");
}

#[test]
fn attach_two_process() {
    if std::env::var("STARDUST_W16_CHILD").ok().as_deref() == Some("1") {
        child_attach();
        return;
    }

    if !device_available() {
        eprintln!("skipping attach_two_process: stardust device not available");
        return;
    }

    let ir = demo_ir();
    let mut region = Region::map_module("w16", 64, 8).expect("map_module w16");
    assert_eq!(region.name(), "w16");
    store_fixture(&ir, &mut region);
    assert_demo(&ir, &region);

    let ptr_a = region.as_ptr();
    assert!(!ptr_a.is_null());

    let exe = std::env::current_exe().expect("current_exe");
    let output = std::process::Command::new(&exe)
        .arg("attach_two_process")
        .arg("--exact")
        .arg("--nocapture")
        .env("STARDUST_W16_CHILD", "1")
        .env("STARDUST_W16_PTR", (ptr_a as usize).to_string())
        .output()
        .expect("spawn w16 child");
    assert!(
        output.status.success(),
        "child attach_two_process failed ({:?})\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        region.as_bytes()[8..24].iter().all(|&b| b == 0xA1),
        "child write_a must be visible through parent mapping"
    );
    assert_eq!(
        region.as_ptr(),
        ptr_a,
        "as_ptr() identity must be unchanged"
    );

    let dup = Region::map_module("w16", 64, 8);
    assert!(
        matches!(dup, Err(Error::MapFailed(_))),
        "duplicate CREATE while live must be MapFailed, got {dup:?}"
    );

    drop(region);
}
