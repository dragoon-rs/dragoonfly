# launch the application
export def main []: nothing -> nothing {
    ^cargo run

    null
}

const DEFAULT_URL = {
    scheme: "http",
    host: "127.0.0.1",
    port: 3000,
}

def run-command []: string -> any {
    let command = $in
    $DEFAULT_URL | insert path ('/' + $command) | url join | http get $in
}

# start to listen on a multiaddr
#
# # Examples
#     listen on 127.0.0.1 and TCP port 31000
#     > app listen "/ip4/127.0.0.1/tcp/31000"
export def listen [
    multiaddr: string, # the multi-address to listen to
]: nothing -> nothing {
    let multiaddr = $multiaddr | str replace --all '/' '%2F'

    $"listen/($multiaddr)" | run-command

    null
}

# get the list of currently connected listeners
export def get-listeners []: nothing -> list<string> {
    "get-listeners" | run-command
}

# get the list of currently connected listeners
export def get-peer-id []: nothing -> list<string> {
    "get-peer-id" | run-command
}
