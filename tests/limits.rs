use itero::workflow::{self, format::Then};

fn flow(extra: &str) -> String {
    format!(
        r#"{{ "name": "sayac-yok", "description": "sayaç yok", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
             {extra}
             "transitions": [ {{ "from": "basla", "to": "bitti" }} ] }}"#
    )
}

#[test]
fn concurrency_and_pause_after_read_and_default() {
    let f = workflow::parse(&flow(
        r#""concurrency": { "max": 1, "then": "skip" }, "pause_after": 3,"#,
    ))
    .unwrap();
    let c = f.concurrency.as_ref().unwrap();
    assert_eq!((c.max, c.then), (1, Then::Skip));
    assert_eq!(f.pause_after_or_default(), 3);
    let f = workflow::parse(&flow(r#""concurrency": { "max": 2 },"#)).unwrap();
    assert_eq!(f.pause_after_or_default(), 5);
    assert_eq!(
        f.concurrency.unwrap().then,
        Then::Queue,
        "queue unless said"
    );
}

#[test]
fn a_concurrency_of_zero_or_an_unknown_then_is_an_error() {
    let e = workflow::parse(&flow(r#""concurrency": { "max": 0 },"#)).unwrap_err();
    assert!(
        e.to_string().contains("concurrency.max is at least 1"),
        "{e}"
    );
    let e =
        workflow::parse(&flow(r#""concurrency": { "max": 1, "then": "bekleş" },"#)).unwrap_err();
    assert!(e.to_string().contains("bekleş"), "{e}");
}

#[test]
fn limits_are_not_part_of_the_version() {
    let a = workflow::parse(&flow("")).unwrap();
    let b = workflow::parse(&flow(r#""concurrency": { "max": 1 }, "pause_after": 0,"#)).unwrap();
    assert_eq!(workflow::version(&a), workflow::version(&b));
}
