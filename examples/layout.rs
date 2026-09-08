use stardust::{Endianness, Ir, Path, Region, Scalar, StructBuilder, Type, Value};

fn main() {
    let demo = StructBuilder::new()
        .field("id", Type::scalar(Scalar::U32))
        .field(
            "vals",
            Type::array(Type::scalar(Scalar::F64), 2).expect("array"),
        )
        .field("tail", Type::slice(Type::scalar(Scalar::U8)))
        .repr_c()
        .expect("repr_c");

    let mut ir = Ir::new(Endianness::Little);
    ir.insert("Demo", demo);

    println!("{}", ir.to_json_pretty().expect("json"));

    let mut region = Region::allocate("demo", 64, 8).expect("alloc");
    ir.store("Demo", &mut region, &Path::new().name("id"), &Value::U32(7))
        .expect("store id");
    ir.store(
        "Demo",
        &mut region,
        &Path::new().name("vals"),
        &Value::Array(vec![Value::F64(1.0), Value::F64(2.5)]),
    )
    .expect("store vals");
    ir.store(
        "Demo",
        &mut region,
        &Path::new().name("tail"),
        &Value::Slice {
            offset: 48,
            elems: vec![Value::U8(9), Value::U8(8), Value::U8(7)],
        },
    )
    .expect("store tail");

    let id = ir
        .retrieve("Demo", &region, &Path::new().name("id"))
        .expect("retrieve id");
    println!("retrieve id = {id:?}");

    {
        let mut view = ir.view("Demo", &mut region).expect("view");
        view.set(&Path::new().name("id"), &Value::U32(11))
            .expect("view set");
        let via_view = view.get(&Path::new().name("id")).expect("view get");
        println!("view id = {via_view:?}");
    }

    let after = ir
        .retrieve("Demo", &region, &Path::new().name("id"))
        .expect("retrieve after view");
    println!("retrieve after view set = {after:?}");
}
