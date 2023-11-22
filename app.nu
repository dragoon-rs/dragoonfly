use std log

const HTTP = {
    OK: 200,
    NOT_FOUND: 404,
}

const DEFAULT_IP = "127.0.0.1:3000"

def run-command [node: string]: string -> any {
    let command = $in

    let res = $node
        | parse "{ip}:{port}"
        | into record
        | rename --column {ip: host}
        | insert scheme "http"
        | insert path $command
        | url join
        | http get $in --allow-errors --full

    if $res.status == $HTTP.NOT_FOUND {
        error make --unspanned {
            msg: $"command `($command)` does not appear to be valid \(($res.status)\)"
        }
    } else if $res.status != $HTTP.OK {
        error make --unspanned {
            msg: $"($res.body) \(($res.status)\)"
        }
    }

    $res.body
}

# launch the application
#
# # Examples
#     start a local swarm with two nodes, using Tmux
#     > app --spawn-swarm-with-tmux --swarm [
#         [ip_port, seed];
#         ["127.0.0.1:3000", 0, "/ip4/127.0.0.1/tcp/31200"],
#         ["127.0.0.1:3001", 1, "/ip4/127.0.0.1/tcp/31201"]
#     ]
#
#     kill all the nodes of the local swarm launched with Tmux
#     > app --kill-tmux-swarm target/debug/dragoonfly
export def main [
    --start: string, # the ip+port of a single node to start
    --seed: int = 0, # the seed for the keypair
    --spawn-swarm-with-tmux, # whether to spawn a swarm with Tmux or not (has precedence over --start)
    --swarm: table<ip_port: string, seed: int, multiaddr: string>, # the table of nodes to open
    --kill-tmux-swarm, # whether to kill the node or not (has precedence over --spawn-swarm-with-tmux)
]: nothing -> nothing {
    if $kill_tmux_swarm {
        ^tmux list-panes -F "#{pane_id}" | lines | reverse | each { ^tmux kill-pane -t $in }
    } else if $spawn_swarm_with_tmux != null and ($swarm | is-empty | not $in) {
        ^tmux new-window nu --execute $'
            $env.PROMPT_COMMAND = "SWARM-CONTROL-PANEL"
            $env.NU_LOG_LEVEL = "DEBUG"
            use app.nu
            const SWARM = ($swarm | to nuon)
        '
        ^tmux split-window -h nu --execute $'
            use app.nu; app --start ($swarm.0.ip_port) --seed ($swarm.0.seed)
        '
        $swarm | skip 1 | each {|it|
            ^tmux select-layout even-vertical
            ^tmux split-window nu --execute $'
                use app.nu; app --start ($it.ip_port) --seed ($it.seed)
            '
        }
        ^tmux select-pane -t (^tmux list-panes -F "#{pane_id}" | lines | first)
    } else if $start != null {
        ^cargo run -- $start $seed
    } else {
        print (help app)
    }

    null
}

# start to listen on a multiaddr
#
# # Examples
#     listen on 127.0.0.1 and TCP port 31000
#     > app listen "/ip4/127.0.0.1/tcp/31000"
export def listen [
    multiaddr: string, # the multi-address to listen to
    --node: string = $DEFAULT_IP
]: nothing -> string {
    log debug $"($node) listening on ($multiaddr)..."
    let multiaddr = $multiaddr | str replace --all '/' '%2F'

    $"listen/($multiaddr)" | run-command $node
}

# get the list of currently connected listeners
export def get-listeners [--node: string = $DEFAULT_IP]: nothing -> list<string> {
    log debug $"getting listeners of ($node)"
    "get-listeners" | run-command $node
}

# get the peer ID of the server in base 58
export def get-peer-id [--node: string = $DEFAULT_IP]: nothing -> string {
    log debug $"getting peer-id of ($node)"
    "get-peer-id" | run-command $node
}

# get some information about the network
export def get-network-info [--node: string = $DEFAULT_IP]: nothing -> record<peers: int, pending: int, connections: int, established: int, pending_incoming: int, pending_outgoing: int, established_incoming: int, established_outgoing: int> {
    log debug $"getting network info of ($node)"
    "get-network-info" | run-command $node
}

# remove a listener from it's ID
#
# Examples
#     remove a listener directly
#     > app remove-listener (app listen "/ip4/127.0.0.1/tcp/31200")
export def remove-listener [
    listener_id: string # the idea of the listener, namely the one given by `listen`
    --node: string = $DEFAULT_IP
]: nothing -> bool {
    log debug $"removing listener ($listener_id) from ($node)"
    $"remove-listener/($listener_id)" | run-command $node
}

# get the list of currently connected peers
export def get-connected-peers [--node: string = $DEFAULT_IP]: nothing -> list<string> {
    log debug $"getting connected peers for ($node)"
    "get-connected-peers" | run-command $node
}

export def dial [
    multiaddr: string, # the multi-address to dial
    --node: string = $DEFAULT_IP
]: nothing -> string {
    log debug $"dialing ($multiaddr) from ($node)"
    let multiaddr = $multiaddr | str replace --all '/' '%2F'

    $"dial/($multiaddr)" | run-command $node
}

export def add-peer [
    multiaddr: string, # the multi-address to add as a peer
    --node: string = $DEFAULT_IP
]: nothing -> string {
    log debug $"adding peer ($multiaddr) to ($node)"
    let multiaddr = $multiaddr | str replace --all '/' '%2F'

    $"add-peer/($multiaddr)" | run-command $node
}

export def start-provide [
    key: string,
    --node: string = $DEFAULT_IP
]: nothing -> any {
    log debug $"($node) starts providing ($key)"
    $"start-provide/($key)" | run-command $node
}

export def get-providers [
    key: string,
    --node: string = $DEFAULT_IP
]: nothing -> any {
    log debug $"getting providers of ($key) from ($node)"
    $"get-providers/($key)" | run-command $node
}

export def bootstrap [
    --node: string = $DEFAULT_IP
]: nothing -> any {
    log debug $"bootstrapping ($node)"
    "bootstrap" | run-command $node
}
