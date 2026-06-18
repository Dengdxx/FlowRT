// FlowRT 管理产物。不要手工修改。

fn main() {
    let mut args = std::env::args().skip(1);
    let mut process = None;
    let mut run_ticks = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--process" => process = args.next(),
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
                eprintln!("unknown FlowRT app argument: {arg}");
                std::process::exit(2);
            }
        }
    }

    let status = match process.as_deref() {
        Some(process) => flowrt_app::runtime_shell::run_process(process, run_ticks),
        None => flowrt_app::runtime_shell::run(run_ticks),
    };
    let code = match status {
        flowrt::Status::Ok => 0,
        _ => 1,
    };
    std::process::exit(code);
}
