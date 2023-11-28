def "nu-complete log-levels" []: nothing -> list<string> {
    [
        "TRACE"
        "DEBUG",
        "INFO",
        "WARN",
        "ERROR",
    ]
}

export def "sap prove" [
    bytes: string,
    k: int,
    n: int,
    --generate-powers,
    --powers-file: path = "powers.bin",
    --log-level: string@"nu-complete log-levels" = "INFO"
] {
    with-env {RUST_LOG: $log_level} {
        ^cargo run [
            --quiet -p semi-avid-pc
            --
            $bytes $k $n ($generate_powers | into string) $powers_file "false"
        ]
    }
}

export def "sap verify" [
    ...blocks: path,
    --generate-powers,
    --powers-file: path = "powers.bin",
    --log-level: string@"nu-complete log-levels" = "INFO"
] {
    with-env {RUST_LOG: $log_level} {
        ^cargo run ([
            --quiet -p semi-avid-pc
            --
            "" 0 0 ($generate_powers | into string) $powers_file "true"
        ] | append $blocks)
    }
}
