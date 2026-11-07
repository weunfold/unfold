

use std::path::Path;

fn exe() -> std::path::PathBuf {
    std::env::current_exe().and_then(std::fs::canonicalize).unwrap_or_else(|e| {
        eprintln!("rth-activate: {e}");
        std::process::exit(1)
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fail = |e: std::io::Error| -> ! {
        eprintln!("rth-activate: {e}");
        std::process::exit(1)
    };
    match (args.get(1).map(String::as_str), args.get(2), args.get(3)) {
        (Some("--effect-helper"), _, _) => rth_activation::effect::helper_main(&args[2..]),
        (Some("--boot-once"), Some(agentd), None) => rth_activation::boot::boot_once(Path::new(agentd)).unwrap_or_else(|e| fail(e)),
        (Some("--verify-boot"), Some(agentd), Some(out)) => rth_activation::boot::verify_boot(Path::new(agentd), Path::new(out)).unwrap_or_else(|e| fail(e)),
        (Some("--birth"), Some(tool), Some(nonce)) if args.len() == 4 => fail(rth_activation::product::birth_main(Path::new(tool), Path::new(nonce))),
        (Some("status"), Some(data), None) => rth_activation::cli::status(Path::new(data)),
        (Some("activate"), Some(data), Some(plan)) if args.len() == 5 => rth_activation::cli::activate(&exe(), Path::new(data), Path::new(plan), Path::new(&args[4])),
        (Some("rollback"), Some(data), Some(plan)) if args.len() == 4 => rth_activation::cli::rollback(&exe(), Path::new(data), Path::new(plan)),
        _ => {
            eprintln!("usage: rth-activate --effect-helper <fence> -- <command…> | --boot-once <agentd> | --verify-boot <agentd> <out.json> | --birth <rth-memory-birth> <nonce-file>  | status <data> | activate <data> <plan.json> <product.json> | rollback <data> <plan.json>");
            std::process::exit(64);
        }
    }
}
