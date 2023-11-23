use std log

const NAME = "dragoonfly"
const LOG_DIR = ($nu.temp-path | path join $NAME)

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
]: nothing -> nothing {
    if ($swarm | is-empty) {
        error make --unspanned {
            msg: "`swarm create` requires a non empty swarm"
        }
    }

    let log_dir = $LOG_DIR | path join (random uuid)

    log info $"logging to `($log_dir)/*.log`"

    ^cargo build --release
    mkdir $log_dir
    for node in $swarm {
        # FIXME: don't use Bash here
        log info $"launching node ($node.seed) \(($node.ip_port)\)"
        ^bash -c $"
            cargo run -- ($node.ip_port) ($node.seed) 1> ($log_dir)/($node.seed).log 2> /dev/null &
        "
    }
    ^$nu.current-exe --execute $'
       $env.PROMPT_COMMAND = "SWARM-CONTROL-PANEL"
       $env.NU_LOG_LEVEL = "DEBUG"
       $env.SWARM_LOG_DIR = ($log_dir)
       use app.nu
       use swarm.nu ["swarm kill", "swarm list", "swarm log"]
       const SWARM = ($swarm | to nuon)
    '

    null
}

# list the nodes of the swarm
export def "swarm list" []: nothing -> table {
    ps | where name =~ $NAME
}

export def "swarm log" [id: int]: nothing -> string {
    let log_file = $env.SWARM_LOG_DIR | path join $"($id).log"

    if not ($log_file | path exists) {
        error make {
            msg: $"(ansi red_bold)invalid_node_id(ansi reset)"
            label: {
                text: $"expected an id between 0 and (swarm list | length | $in - 1), found ($id)"
                span: (metadata $id).span
            }
        }
    }

    open $log_file
}

# kill the swarm
export def "swarm kill" []: nothing -> nothing {
    ps | where name =~ $NAME | each {|it|
        print $"killing ($it.pid)"
        kill $it.pid
    }

    exit
}
