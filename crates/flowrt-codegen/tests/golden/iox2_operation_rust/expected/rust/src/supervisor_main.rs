// FlowRT 管理产物。不要手工修改。

fn main() {
    let mut args = std::env::args().skip(1);
    let mut run_ticks = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--flowrt-run-ticks" | "--flowrt-run-steps" => {
                let Some(raw_ticks) = args.next() else {
                    eprintln!("missing value for {arg}");
                    std::process::exit(2);
                };
                match raw_ticks.parse::<usize>() {
                    Ok(ticks) if ticks > 0 => run_ticks = Some(ticks),
                    _ => {
                        eprintln!("invalid value for {arg}: {raw_ticks}");
                        std::process::exit(2);
                    }
                }
            }
            _ => {
                eprintln!("unknown FlowRT supervisor argument: {arg}");
                std::process::exit(2);
            }
        }
    }

    match flowrt_app::supervisor::launch(run_ticks) {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("FlowRT supervisor failed: {error}");
            std::process::exit(1);
        }
    }
}
