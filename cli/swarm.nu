use std log

const NAME = "dragoonfly"
const LOG_DIR = ($nu.temp-path | path join $NAME)
const POWERS_PATH = "setup/powers/powers_test_Fr_155kB"

# create a swarm table
export def "swarm create" [n: int]: nothing -> table {
    seq 0 ($n - 1) | each { {
        ip_port: $"127.0.0.1:(3_000 + $in)",
        seed: $in,
        multiaddr: $"/ip4/127.0.0.1/tcp/(31_200 + $in)",
    } }
}

# run a swarm
export def "swarm run" [
    swarm: table<ip_port: string, seed: int, multiaddr: string>, # the table of nodes to run
    --features: list<string> = [], # features to include in the nodes
    --no-shell
]: nothing -> string {
    if ($swarm | is-empty) {
        error make --unspanned {
            msg: "`swarm create` requires a non empty swarm"
        }
    }

    let log_dir = $LOG_DIR | path join (random uuid)

    log info $"logging to `($log_dir)/*.log`"

    ^cargo build --release --features ($features | str join ",") 
    mkdir $log_dir
    for node in $swarm {
        # FIXME: don't use Bash here
        log info $"launching node ($node.seed) \(($node.ip_port)\)"
        ^bash -c (
            $"cargo run --features '($features | str join ',')' "
          + $"-- ($POWERS_PATH) ($node.ip_port) ($node.seed) "
          + $"1> ($log_dir)/($node.seed).log 2> /dev/null &"
        )
    }

    if not $no_shell {
        ^$nu.current-exe --execute $'
       $env.PROMPT_COMMAND = "SWARM-CONTROL-PANEL"
       $env.NU_LOG_LEVEL = "DEBUG"
       $env.SWARM_LOG_DIR = ($log_dir)
       use cli/app.nu
       use cli/swarm.nu ["swarm kill", "swarm list", "swarm log", "bytes decode"]
       const SWARM = ($swarm | to nuon)
    '
    }

    $log_dir
}

# list the nodes of the swarm
export def "swarm list" []: nothing -> table {
    ps | where name =~ $NAME
}

def parse-tracing-logs []: string -> table<date: datetime, level: string, id: int, file: string, msg: string> {
     lines
        | ansi strip
        | parse --regex '^(?<date>.{27}) (?<level>.{5}) (?<file>[\w:_-]*): (?<msg>.*)'
        | str trim level
        | into datetime date
}

export def "swarm log" []: nothing -> table<date: datetime, level: string, id: int, file: string, msg: string> {
    # FIXME: this should not require `mut`
    # related to https://github.com/nushell/nushell/issues/10428
    mut logs = []
    for id in (seq 0 (swarm list | length | $in - 1)) {
        let log_file = $env.SWARM_LOG_DIR | path join $"($id).log"
        if not ($log_file | path exists) {
            log warning $"`($log_file)` does not exist"
            continue
        }

        let log = open $log_file --raw | parse-tracing-logs | insert id $id | move id --before file
        $logs = ($logs | append $log)
    }

    let logs = $logs | sort-by date

    if not ($logs | is-empty) {
        let start = $logs.0.date
        $logs | update date { $in - $start }
    } else {
        []
    }
}

# kill the swarm
export def "swarm kill" [--no-shell]: nothing -> nothing {
    ps | where name =~ $NAME | each {|it|
        log warning $"killing ($it.pid)"
        kill $it.pid
    }
    if not $no_shell {
        exit
    }
}

# decode a list of integer bytes into the underlying encoded string
export def "bytes decode" [encoding: string = "utf-8"]: list<int> -> string {
    each { into binary | bytes at 0..1 } | bytes collect | decode $encoding
}

# encode an encoded string into the underlying list of integer bytes
export def "bytes encode" [encoding: string = "utf-8"]: string -> list<int> {
    let bytes = $in | encode $encoding
    seq 1 ($bytes | bytes length) | each {|i|
        $bytes | bytes at ($i - 1)..($i) | into int
    }
}
