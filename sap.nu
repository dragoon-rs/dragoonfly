def "nu-complete log-levels" []: nothing -> list<string> {
    [
        "TRACE"
        "DEBUG",
        "INFO",
        "WARN",
        "ERROR",
    ]
}

def run-sap [
    args: record<bytes: string, k: int, n: int, do_generate_powers: bool, powers_file: path, do_reconstruct_data: bool, do_verify_blocks: bool, block_files: list<string>>,
    --log-level: string,
]: nothing -> any {
    with-env {RUST_LOG: $log_level} {
        let res = do {
            ^./target/release/semi-avid-pc ([
                $args.bytes
                $args.k
                $args.n
                ($args.do_generate_powers | into string)
                $args.powers_file
                ($args.do_reconstruct_data | into string)
                ($args.do_verify_blocks | into string)
            ] | append $args.block_files)
        } | complete

        print $res.stdout
        $res.stderr | from json
    }
}

export def "sap build" [] {
    ^cargo build --package semi-avid-pc --release
}

export def "sap setup" [
    bytes: string,
    --powers-file: path = "powers.bin",
    --log-level: string@"nu-complete log-levels" = "INFO"
]: nothing -> nothing {
    run-sap --log-level $log_level {
        bytes: $bytes,
        k: 0,
        n: 0,
        do_generate_powers: true,
        powers_file: $powers_file,
        do_reconstruct_data: false,
        do_verify_blocks: false,
        block_files: [],
    }
}

export def "sap prove" [
    bytes: string,
    k: int,
    n: int,
    --powers-file: path = "powers.bin",
    --log-level: string@"nu-complete log-levels" = "INFO"
]: nothing -> list<string> {
    run-sap --log-level $log_level {
        bytes: $bytes,
        k: $k,
        n: $n,
        do_generate_powers: false,
        powers_file: $powers_file,
        do_reconstruct_data: false,
        do_verify_blocks: false,
        block_files: [],
    }
}

export def "sap verify" [
    ...blocks: path,
    --powers-file: path = "powers.bin",
    --log-level: string@"nu-complete log-levels" = "INFO"
]: nothing -> table<block: string, status: int> {
    run-sap --log-level $log_level {
        bytes: "",
        k: 0,
        n: 0,
        do_generate_powers: false,
        powers_file: $powers_file,
        do_reconstruct_data: false,
        do_verify_blocks: true,
        block_files: $blocks,
    }
}

export def "sap reconstruct" [
    ...blocks: path,
    --log-level: string@"nu-complete log-levels" = "INFO"
]: nothing -> list<int> {
    run-sap --log-level $log_level {
        bytes: "",
        k: 0,
        n: 0,
        do_generate_powers: false,
        powers_file: "",
        do_reconstruct_data: true,
        do_verify_blocks: false,
        block_files: $blocks,
    }
}
