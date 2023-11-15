# launch the application
export def main []: nothing -> nothing {
    ^cargo run

    null
}

const HTTP_OK = 200

const DEFAULT_URL = {
    scheme: "http",
    host: "127.0.0.1",
    port: 3000,
}

def run-command []: string -> any {
    let command = $in
    $DEFAULT_URL | insert path $command | url join | http get $in --allow-errors --full
}

# start to listen on a multiaddr
#
# # Examples
#     listen on 127.0.0.1 and TCP port 31000
#     > app listen "/ip4/127.0.0.1/tcp/31000"
export def listen [
    multiaddr: string, # the multi-address to listen to
]: nothing -> string {
    let multiaddr = $multiaddr | str replace --all '/' '%2F'

    let res = $"listen/($multiaddr)" | run-command
    if $res.status != $HTTP_OK {
        error make --unspanned {
            msg: $"($res.body) \(($res.status)\)"
        }
    }

    $res.body | parse "ListenerId({id})" | into record | get id
}

# get the list of currently connected listeners
export def get-listeners []: nothing -> list<string> {
    "get-listeners" | run-command
}

# get the peer ID of the server in base 58
export def get-peer-id []: nothing -> string {
    "get-peer-id" | run-command
}

# get some information about the network
export def get-network-info []: nothing -> record<peers: int, pending: int, connections: int, established: int, pending_incoming: int, pending_outgoing: int, established_incoming: int, established_outgoing: int> {
    "get-network-info" | run-command
}

# remove a listener from it's ID
#
# Examples
#     remove a listener directly
#     > app remove-listener (app listen "/ip4/127.0.0.1/tcp/31200")
export def remove-listener [
    listener_id: string # the idea of the listener, namely the one given by `listen`
]: nothing -> bool {
    $"remove-listener/($listener_id)" | run-command
}
