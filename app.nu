const HTTP_OK = 200

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
    if $res.status != $HTTP_OK {
        error make --unspanned {
            msg: $"($res.body) \(($res.status)\)"
        }
    }

    $res.body
}

# launch the application
export def main [--start: string]: nothing -> nothing {
    if $start != null {
        ^cargo run -- $start
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
    let multiaddr = $multiaddr | str replace --all '/' '%2F'

    $"listen/($multiaddr)" | run-command $node
}

# get the list of currently connected listeners
export def get-listeners [--node: string = $DEFAULT_IP]: nothing -> list<string> {
    "get-listeners" | run-command $node
}

# get the peer ID of the server in base 58
export def get-peer-id [--node: string = $DEFAULT_IP]: nothing -> string {
    "get-peer-id" | run-command $node
}

# get some information about the network
export def get-network-info [--node: string = $DEFAULT_IP]: nothing -> record<peers: int, pending: int, connections: int, established: int, pending_incoming: int, pending_outgoing: int, established_incoming: int, established_outgoing: int> {
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
    $"remove-listener/($listener_id)" | run-command $node
}

# get the list of currently connected peers
export def get-connected-peers [--node: string = $DEFAULT_IP]: nothing -> list<string> {
    "get-connected-peers" | run-command $node
}

export def dial [
    multiaddr: string, # the multi-address to dial
    --node: string = $DEFAULT_IP
]: nothing -> string {
    let multiaddr = $multiaddr | str replace --all '/' '%2F'

    $"dial/($multiaddr)" | run-command $node
}


export def add-peer [
    multiaddr: string, # the multi-address to add as a peer
    --node: string = $DEFAULT_IP
]: nothing -> string {
    let multiaddr = $multiaddr | str replace --all '/' '%2F'

    $"add-peer/($multiaddr)" | run-command $node
}
