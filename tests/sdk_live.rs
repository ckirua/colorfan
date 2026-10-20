use std::time::Instant;
use colorfan::sdk::SdkClient;

#[test]
fn sdk_list_and_apply() {
    if std::env::var("COLORFAN_LIVE").is_err() {
        eprintln!("skip (set COLORFAN_LIVE=1)");
        return;
    }
    let mut c = SdkClient::connect().expect("connect");
    let ctrls = c.list_controllers().expect("list");
    assert!(!ctrls.is_empty());
    eprintln!("controllers: {:#?}", ctrls);
    let id = ctrls[0].id;
    let zones: Vec<(u32, String)> = ctrls[0]
        .zones
        .iter()
        .enumerate()
        .map(|(i, _)| (i as u32, if i == 1 { "00FF00".into() } else { "000000".into() }))
        .collect();
    let refs: Vec<(u32, &str)> = zones.iter().map(|(z, h)| (*z, h.as_str())).collect();
    let start = Instant::now();
    c.apply_zones(id, &refs).expect("apply");
    eprintln!("SDK apply took {:?}", start.elapsed());
}
