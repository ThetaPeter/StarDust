use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use stardust::{
    Advice, CallArg, Endianness, Error, Import, Ir, Library, Param, Path, Region, Ret, Scalar,
    SlotAlloc, StructBuilder, Type, Value,
};

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

// R1
struct DeleteOnDrop(PathBuf);

impl Drop for DeleteOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temp_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "stardust-w4-{}-{}.bin",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

#[test]
fn mmap_c() {
    let ir = ab_ir();
    let lib = load_fixture();
    let func_a = lib.bind(&ir, "write_a").unwrap();
    let func_b = lib.bind(&ir, "mut_b").unwrap();

    let mut region = Region::map_anon("w4", 32, 8).unwrap();
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
        "span [8,24) must be 0xA2 after write_a then mut_b, got {:?}",
        &after[SPAN]
    );
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");
}

#[test]
fn file_round_trip() {
    let path = temp_path();
    let _cleanup = DeleteOnDrop(path.clone());

    let ty = StructBuilder::new()
        .field("tag", Type::scalar(Scalar::I32))
        .repr_c()
        .unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Item", ty);
    let tag = Path::new().name("tag");

    {
        let mut region = Region::map_file("w4", &path, 64, 8).unwrap();
        ir.store("Item", &mut region, &tag, &Value::I32(-7))
            .unwrap();
    }

    let region = Region::open_file("w4", &path, 8).unwrap();
    assert_eq!(ir.retrieve("Item", &region, &tag).unwrap(), Value::I32(-7));
}

#[test]
fn view_on_map_anon() {
    let ty = StructBuilder::new()
        .field("tag", Type::scalar(Scalar::I32))
        .repr_c()
        .unwrap();
    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Item", ty);
    let tag = Path::new().name("tag");

    let mut region = Region::map_anon("w4-view", 32, 8).unwrap();
    let base = region.as_ptr();
    ir.store("Item", &mut region, &tag, &Value::I32(42))
        .unwrap();
    {
        let mut view = ir.view("Item", &mut region).unwrap();
        assert_eq!(view.as_ptr(), base);
        assert_eq!(view.get(&tag).unwrap(), Value::I32(42));
        view.set(&tag, &Value::I32(7)).unwrap();
    }
    assert_eq!(ir.retrieve("Item", &region, &tag).unwrap(), Value::I32(7));
    assert_eq!(region.as_ptr(), base);
}

// R2
#[cfg(windows)]
fn assert_windows_unsupported(err: Error, family: &'static str) {
    assert!(
        matches!(err, Error::Unsupported { family: f } if f == family),
        "{family} must be Error::Unsupported, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains(family),
        "Unsupported Display must name {family}, got {msg}"
    );
}

#[cfg(unix)]
fn assert_unix_page_or_ok(result: stardust::Result<()>, family: &str) {
    match result {
        Ok(()) => {}
        Err(Error::PageControl(_)) => {}
        Err(e) => panic!("{family} must be Ok or PageControl, got {e:?}"),
    }
}

#[test]
fn pin() {
    let region = Region::map_anon("w5", 32, 8).unwrap();
    match region.pin() {
        Ok(()) => {}
        Err(Error::PinFailed(_)) => {}
        Err(Error::Unsupported { .. }) => {
            panic!("pin must invoke mlock/VirtualLock; Unsupported is not allowed")
        }
        Err(e) => panic!("pin must be Ok or PinFailed, got {e:?}"),
    }
}

#[test]
fn advice() {
    let region = Region::map_anon("w5", 32, 8).unwrap();
    let result = region.advise(Advice::WillNeed);
    #[cfg(windows)]
    assert_windows_unsupported(
        result.expect_err("Windows advice must be Unsupported"),
        "advice",
    );
    #[cfg(unix)]
    assert_unix_page_or_ok(result, "advice");
}

#[test]
fn huge() {
    let region = Region::map_anon("w5", 32, 8).unwrap();
    let result = region.request_huge();
    #[cfg(windows)]
    assert_windows_unsupported(
        result.expect_err("Windows huge must be Unsupported"),
        "huge",
    );
    #[cfg(unix)]
    assert_unix_page_or_ok(result, "huge");
}

#[test]
fn numa() {
    let region = Region::map_anon("w5", 32, 8).unwrap();
    let result = region.bind_numa(0);
    #[cfg(windows)]
    assert_windows_unsupported(
        result.expect_err("Windows NUMA must be Unsupported"),
        "numa",
    );
    #[cfg(unix)]
    assert_unix_page_or_ok(result, "numa");
}

#[test]
fn pin_mmap_c() {
    let ir = ab_ir();
    let lib = load_fixture();
    let func_a = lib.bind(&ir, "write_a").unwrap();
    let func_b = lib.bind(&ir, "mut_b").unwrap();

    let mut region = Region::map_anon("w5", 32, 8).unwrap();
    fill_non_a1(&mut region);
    let before = region.as_bytes().to_vec();

    let pin_ok = match region.pin() {
        Ok(()) => true,
        Err(Error::PinFailed(_)) => false,
        Err(Error::Unsupported { .. }) => {
            panic!("pin must not return Unsupported")
        }
        Err(e) => panic!("unexpected pin error: {e:?}"),
    };
    if !pin_ok {
        eprintln!("pin failed with PinFailed; skipping C in-place check");
        return;
    }

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
        "span [8,24) must be 0xA2 after write_a then mut_b, got {:?}",
        &after[SPAN]
    );
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");
}

#[test]
fn slot_reuse() {
    let ty = Type::scalar(Scalar::U32);
    let size = ty.inline_size();

    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import("write_a", ptr_len_void("write_a"))
        .unwrap();
    let lib = load_fixture();
    let write_a = lib.bind(&ir, "write_a").unwrap();

    let mut region = Region::allocate("r6", 32, 8).unwrap();
    fill_non_a1(&mut region);
    let base = region.as_ptr();
    let before = region.as_bytes().to_vec();

    let (_a, b, c) = {
        let mut slots = SlotAlloc::new(&mut region);
        let a = slots.alloc(&ty).unwrap();
        let b = slots.alloc(&ty).unwrap();
        assert_ne!(b, a, "second live slot must differ from the first");
        slots.free(a).unwrap();
        assert!(
            matches!(slots.free(a), Err(Error::OutOfBounds)),
            "double-free of A must be OutOfBounds"
        );
        assert!(
            matches!(slots.free(0), Err(Error::OutOfBounds)),
            "free(0) must be OutOfBounds"
        );
        let c = slots.alloc(&ty).unwrap();
        assert_eq!(c, a, "same Type after free A with B live reuses A");
        (a, b, c)
    };

    let b_start = usize::try_from(b).unwrap();
    let b_end = b_start + usize::try_from(size).unwrap();
    let c_start = usize::try_from(c).unwrap();
    let c_end = c_start + usize::try_from(size).unwrap();
    let b_before_write = region.as_bytes()[b_start..b_end].to_vec();
    assert_eq!(
        &b_before_write[..],
        &before[b_start..b_end],
        "alloc must not write B's span"
    );

    write_a
        .call(
            &mut region,
            &[
                CallArg::Ptr {
                    region: 0,
                    offset: c,
                },
                CallArg::Len(size),
            ],
        )
        .unwrap();

    assert_eq!(
        region.as_ptr(),
        base,
        "as_ptr() identity must be unchanged through write_a on C"
    );
    let after = region.as_bytes();
    assert!(
        after[c_start..c_end].iter().all(|&byte| byte == 0xA1),
        "C span must be 0xA1 after write_a, got {:?}",
        &after[c_start..c_end]
    );
    assert_eq!(
        &after[b_start..b_end],
        &b_before_write[..],
        "B's bytes must be unchanged"
    );
}
