use std log

# launch a swarm
export def "swarm create" [
    swarm: table<ip_port: string, seed: int, multiaddr: string>, # the table of nodes to run
]: nothing -> nothing {
    if ($swarm | is-empty) {
        error make --unspanned {
            msg: "`swarm create` requires a non empty swarm"
        }
    }

    ^cargo build --release
    mkdir /tmp/dragoonfly
    for node in $swarm {
        # FIXME: don't use Bash here
        log info $"launching node ($node.seed) \(($node.ip_port)\)"
        ^bash -c $"
            cargo run -- ($node.ip_port) ($node.seed) 1> /tmp/dragoonfly/($node.seed).log 2> /dev/null &
        "
    }
    ^$nu.current-exe --execute $'
       $env.PROMPT_COMMAND = "SWARM-CONTROL-PANEL"
       $env.NU_LOG_LEVEL = "DEBUG"
       use app.nu
       use swarm.nu "swarm kill"
       const SWARM = ($swarm | to nuon)
    '

    null
}

# list the nodes of the swarm
export def "swarm list" [] {
    ps | where name =~ dragoonfly
}

# kill the swarm
export def "swarm kill" [] {
    ps | where name =~ dragoonfly | each {|it|
        print $"killing ($it.pid)"
        kill $it.pid
    }

    exit
}
