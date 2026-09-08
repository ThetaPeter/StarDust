use stardust::{CallArg, Endianness, Import, Ir, Library, Param, Region, Ret};

fn ptr_len_void(symbol: &str) -> Import {
    Import::new(symbol, vec![Param::Ptr, Param::Len], Ret::Void)
}

fn write_a_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import("write_a", ptr_len_void("write_a"))
        .unwrap();
    ir
}

fn mutate_ir() -> Ir {
    let mut ir = Ir::new(Endianness::Little);
    ir.insert_import("mutate", ptr_len_void("mutate")).unwrap();
    ir
}

fn load_c_fixture() -> Library {
    Library::load(stardust_c::process_lib_path()).expect("load W2 C fixture")
}

fn load_hs_fixture() -> Library {
    Library::load(stardust_hs::mutate_lib_path()).expect("load W10 Haskell fixture")
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
fn haskell_on_region() {
    let mut region = Region::allocate("w10", 32, 8).unwrap();
    fill_non_a1(&mut region);
    let before = region.as_bytes().to_vec();

    let before_ptr = region.as_ptr();
    let c_lib = load_c_fixture();
    let func_a = c_lib.bind(&write_a_ir(), "write_a").unwrap();
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
        before_ptr, after_a,
        "as_ptr() must be unchanged through write_a"
    );
    assert!(
        region.as_bytes()[SPAN].iter().all(|&b| b == 0xA1),
        "span [8,24) must be 0xA1 after write_a, got {:?}",
        &region.as_bytes()[SPAN]
    );

    let hs_lib = load_hs_fixture();
    let func_m = hs_lib.bind(&mutate_ir(), "mutate").unwrap();
    func_m
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
    let after_hs = region.as_ptr();
    assert_eq!(
        after_a, after_hs,
        "as_ptr() must be unchanged through Haskell mutate"
    );
    assert_eq!(
        before_ptr, after_hs,
        "C and Haskell must share the same region base"
    );

    let after = region.as_bytes();
    assert_eq!(&after[..8], &before[..8], "prefix must be unchanged");
    assert!(
        after[SPAN].iter().all(|&b| b == 0xA2),
        "span [8,24) must be 0xA2 after write_a then Haskell wrapping +1, got {:?}",
        &after[SPAN]
    );
    assert_eq!(&after[24..], &before[24..], "suffix must be unchanged");
}
