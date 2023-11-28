def "nu-complete log-levels" []: nothing -> list<string> {
    [
        "TRACE"
        "DEBUG",
        "INFO",
        "WARN",
        "ERROR",
    ]
}

export def "sap setup" [
    bytes: string,
    --powers-file: path = "powers.bin",
    --log-level: string@"nu-complete log-levels" = "INFO"
] {
    with-env {RUST_LOG: $log_level} {
        ^cargo run [
            --quiet -p semi-avid-pc
            --
            $bytes 0 0 "true" $powers_file "false"
        ]
    }
}

export def "sap prove" [
    bytes: string,
    k: int,
    n: int,
    --powers-file: path = "powers.bin",
    --log-level: string@"nu-complete log-levels" = "INFO"
] {
    with-env {RUST_LOG: $log_level} {
        ^cargo run [
            --quiet -p semi-avid-pc
            --
            $bytes $k $n "false" $powers_file "false"
        ]
    }
}

export def "sap verify" [
    ...blocks: path,
    --powers-file: path = "powers.bin",
    --log-level: string@"nu-complete log-levels" = "INFO"
] {
    with-env {RUST_LOG: $log_level} {
        ^cargo run ([
            --quiet -p semi-avid-pc
            --
            "" 0 0 "false" $powers_file "true"
        ] | append $blocks)
    }
}
