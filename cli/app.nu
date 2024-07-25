use std log
use ../komodo/binary.nu "bytes from_int"

const HTTP = {
    OK: 200,
    NOT_FOUND: 404,
}

const DEFAULT_IP = "127.0.0.1:3000"
const POWERS_PATH = "setup/powers/powers_test_Fr_155kB"

# def "http get-curl" [url: string, --allow-errors, --full]: nothing -> record<body: string, status: int> {
#     $url
#         | str replace --all ' ' "%20"
#         | curl -i $in
#         | lines
#         | split list ""
#         | update 0 { get 0 | parse "HTTP/{v} {c} {m}" | into record }
#         | update 1 { get 0 }
#         | {
#             body: ($in.1 | str replace --regex '^"' '' | str replace --regex '"$' '' | from json),
#             status: ($in.0.c | into int),
#         }
# }

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
        #| http get-curl $in --allow-errors --full

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
    let multiaddr = $multiaddr | slash replace

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

export def dial-single [
    multiaddr: string, # the multi-address to dial
    --node: string = $DEFAULT_IP
]: nothing -> string {
    log debug $"dialing ($multiaddr) from ($node)"
    let multiaddr = $multiaddr | slash replace

    $"dial-single/($multiaddr)" | run-command $node
}

export def dial-multiple [
    list_multiaddr: list<string>, # all the multi-addresses to dial
    --node: string = $DEFAULT_IP
]: nothing -> string {
    log debug $"dialing all the following multiaddr: ($list_multiaddr) from ($node)"
    let list_multiaddr = $list_multiaddr | each {slash replace} | to json

    $"dial-multiple/($list_multiaddr)" | run-command $node
}

export def add-peer [
    multiaddr: string, # the multi-address to add as a peer
    --node: string = $DEFAULT_IP
]: nothing -> string {
    log debug $"adding peer ($multiaddr) to ($node)"
    let multiaddr = $multiaddr | slash replace

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

export def add-file [
    key: string,
    content: string,
    --node: string = $DEFAULT_IP
]: nothing -> any {
    log debug $"adding ($content) to ($node)"
    $"add-file/($key)/($content)" | run-command $node
}

export def decode-blocks [
    block_dir: string,
    block_hashes: list<string>,
    output_filename: string,
    --node: string = $DEFAULT_IP,
]: nothing -> any {
    log debug $"decoding the blocks ($block_hashes) from ($block_dir)"
    let message = $"decode-blocks/($block_dir | slash replace)/($block_hashes | to json)/($output_filename)"
    $message | run-command $node
}

export def encode-file [
    file_path: string,
    --powers_path: string = $POWERS_PATH,
    --replace-blocks = true,
    --k: int = 3,
    --n: int = 5,
    --encoding_method: string = Random,
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"encoding the file ($file_path)"
    let file_path_enc = $file_path | slash replace
    let powers_path_enc = ($powers_path | slash replace)
    let list_args = [$file_path_enc, $replace_blocks, $encoding_method, $k, $n, $powers_path_enc]
    $"encode-file/($list_args | str join '/')" | run-command $node
}

export def get-block-from [
    peer_id_base_58: string,
    file_hash: string,
    block_hash: string,
    --node: string = $DEFAULT_IP
] nothing -> any {
    log debug $"get block ($block_hash) part of file ($file_hash) from peer ($peer_id_base_58)"
    $"get-block-from/($peer_id_base_58)/($file_hash)/($block_hash)" | run-command $node | get block_data | bytes from_int
}

def "slash replace" [] string -> string {
    $in | str replace --all '/' '%2F'
}

export def get-file [
    file_hash: string,
    output_filename: string,
    --powers_path: string = $POWERS_PATH,
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"Getting file ($file_hash)"
    $"get-file/($file_hash)/($output_filename)/($powers_path | slash replace)" | run-command $node

}

export def get-blocks-info-from [
    peer_id_base_58: string,
    filename: string,
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"Getting the list of blocks from ($peer_id_base_58) for file ($filename)"
    $"get-blocks-info-from/($peer_id_base_58)/($filename)" | run-command $node
}

export def get-block-list [
    filename: string,
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"Getting the list of blocks for file ($filename) from own node"
    $"get-block-list/($filename)" | run-command $node
}

export def node-info [
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"Getting the info from node ($node)"
    "node-info" | run-command $node
}

export def send-block-to [
    peer_id_base_58: string,
    file_hash: string,
    block_hash: string,
    --node: string = $DEFAULT_IP
] nothing -> any {
    log debug $"Sending block ($block_hash) part of file ($file_hash) to ($peer_id_base_58)"
    $"send-block-to/($peer_id_base_58)/($file_hash)/($block_hash)" | run-command $node
}

export def get-available-storage [
    --node: string = $DEFAULT_IP
] nothing -> any {
    log debug $"Getting the size left available for sending blocks from ($node)"
    $"get-available-storage" | run-command $node
}
