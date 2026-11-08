use rth_activation::lease::{Lease, Mode};
use rth_activation::seam::admit;
use serde_json::json;

#[test]
fn a_refused_admission_releases_its_shared_lease_at_once() {
    let d = std::env::temp_dir().join(format!("rth-admit-release-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let service = json!({"label": "x"});
    rth_activation::record::write(&d.join("activation.json.real"), &json!({"seq": 1, "activation_state": "interim_active", "op_id": "op"}), 0o600).unwrap();

    std::os::unix::fs::symlink(d.join("activation.json.real"), d.join("activation.json")).unwrap();
    assert!(admit(&d, &service).is_err());
    assert!(Lease::acquire(&d.join("rth.op.lock"), Mode::Exclusive).is_ok(), "released at once (link)");
    std::fs::remove_file(d.join("activation.json")).unwrap();
    std::fs::rename(d.join("activation.json.real"), d.join("activation.json")).unwrap();
    std::os::unix::fs::symlink(d.join("nowhere.json"), d.join("rth.op.json")).unwrap();
    assert!(admit(&d, &service).is_err());
    assert!(Lease::acquire(&d.join("rth.op.lock"), Mode::Exclusive).is_ok(), "released at once (dangling)");
}
