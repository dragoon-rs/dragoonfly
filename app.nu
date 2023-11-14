# launch the application
export def main []: nothing -> nothing {
    ^cargo run

    null
}

const DEFAULT_IP = "127.0.0.1"
const DEFAULT_PORT = 3000
const DEFAULT_PROTO = "http"

# start to listen on a multiaddr
#
# # Examples
#     listen on 127.0.0.1 and TCP port 31000
#     > app listen "/ip4/127.0.0.1/tcp/31000"
export def listen [
    multiaddr: string, # the multi-address to listen to
    --ip: string = $DEFAULT_IP, # the IP address of the server
    --port: int = $DEFAULT_PORT, # the port to connect to the server
    --proto: string = $DEFAULT_PROTO # the protocol to connect to the server
]: nothing -> nothing {
    let multiaddr = $multiaddr | str replace --all '/' '%2F'
    let url = {
        scheme: $proto,
        host: $ip,
        port: $port,
        path: $"/listen/($multiaddr)",
    }

    $url | url join | http get $in

    null
}

# get the list of currently connected listeners
export def get-listeners [
    --ip: string = $DEFAULT_IP, # the IP address of the server
    --port: int = $DEFAULT_PORT, # the port to connect to the server
    --proto: string = $DEFAULT_PROTO # the protocol to connect to the server
]: nothing -> list<string> {
    let url = {
        scheme: $proto,
        host: $ip,
        port: $port,
        path: $"/get-listeners",
    }

    $url | url join | http get $in
}

# get the list of currently connected listeners
export def get-peer-id [
    --ip: string = $DEFAULT_IP, # the IP address of the server
    --port: int = $DEFAULT_PORT, # the port to connect to the server
    --proto: string = $DEFAULT_PROTO # the protocol to connect to the server
]: nothing -> list<string> {
    let url = {
        scheme: $proto,
        host: $ip,
        port: $port,
        path: $"/get-peer-id",
    }

    $url | url join | http get $in
}
