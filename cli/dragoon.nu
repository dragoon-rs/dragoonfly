use std log

const HTTP = {
    OK: 200,
    NOT_FOUND: 404,
}

const DEFAULT_IP = "127.0.0.1:3000"
const POWERS_PATH = "setup/powers/powers_test_Fr_155kB"

def "bytes from_int" []: [int -> binary, list<int> -> binary] {
    each { into binary --compact } | bytes collect
}

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

def run-command [
    node: string,
    --post-body: any
]: string -> any {
    let command_path = $in

    let query = $node
        | parse "{ip}:{port}"
        | into record
        | rename --column {ip: host}
        | insert scheme "http"
        | insert path $command_path
        | url join

    let res = if $post_body != null {
        http post --allow-errors --full -t application/json $query $post_body
    } else {
        http get --allow-errors --full $query 
    }

    if $res.status == $HTTP.NOT_FOUND {
        error make --unspanned {
            msg: $"command `($command_path)` does not appear to be valid \(($res.status)\): ($res.body)"
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
#     > dragoon listen "/ip4/127.0.0.1/tcp/31000"
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

# get some information about the network
export def get-network-info [--node: string = $DEFAULT_IP]: nothing -> record<peers: int, pending: int, connections: int, established: int, pending_incoming: int, pending_outgoing: int, established_incoming: int, established_outgoing: int> {
    log debug $"getting network info of ($node)"
    "get-network-info" | run-command $node
}

# remove a listener from it's ID
#
# Examples
#     remove a listener directly
#     > dragoon remove-listener (dragoon listen "/ip4/127.0.0.1/tcp/31200")
export def remove-listener [
    listener_id: string # the idea of the listener, namely the one given by `listen`
    --node: string = $DEFAULT_IP
]: nothing -> bool {
    log debug $"removing listener ($listener_id) from ($node)"
    $"remove-listener" | run-command $node --post-body $listener_id
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
    
    $"dial-single" | run-command $node --post-body $multiaddr
}

export def dial-multiple [
    list_multiaddr: list<string>, # all the multi-addresses to dial
    --node: string = $DEFAULT_IP
]: nothing -> string {
    log debug $"dialing all the following multiaddr: ($list_multiaddr) from ($node)"

    if $list_multiaddr == [] {
        return "1"
    }
    
    $"dial-multiple" | run-command $node --post-body $list_multiaddr
}

export def add-peer [
    multiaddr: string, # the multi-address to add as a peer
    --node: string = $DEFAULT_IP
]: nothing -> string {
    log debug $"adding peer ($multiaddr) to ($node)"

    $"add-peer" | run-command $node --post-body $multiaddr
}

export def start-provide [
    file_hash: string,
    --node: string = $DEFAULT_IP
]: nothing -> any {
    log debug $"($node) starts providing ($file_hash)"
    $"start-provide" | run-command $node --post-body $file_hash
}

export def stop-provide [
    key: string,
    --node: string = $DEFAULT_IP
]: nothing -> any {
    log debug $"($node) stops providing ($key)"
    $"stop-provide" | run-command $node --post-body $key
}

export def get-providers [
    file_hash: string,
    --node: string = $DEFAULT_IP
]: nothing -> any {
    log debug $"getting providers of ($file_hash) from ($node)"
    $"get-providers" | run-command $node --post-body $file_hash
}

export def bootstrap [
    --node: string = $DEFAULT_IP
]: nothing -> any {
    log debug $"bootstrapping ($node)"
    "bootstrap" | run-command $node
}

export def decode-blocks [
    block_dir: string,
    block_hashes: list<string>,
    output_filename: string,
    --node: string = $DEFAULT_IP,
]: nothing -> any {
    let block_dir = $block_dir | path expand
    log debug $"decoding the blocks ($block_hashes) from ($block_dir)"
    "decode-blocks" | run-command $node --post-body [$block_dir, $block_hashes, $output_filename]
}

export def encode-file [
    file_path: string,
    --replace-blocks = true,
    --k: int = 3,
    --n: int = 5,
    --encoding-method: string = Random,
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"encoding the file ($file_path)"
    let list_args = [$file_path, $replace_blocks, $encoding_method, $k, $n]
    $"encode-file" | run-command $node --post-body $list_args
}

export def get-block-from [
    peer_id_base_58: string,
    file_hash: string,
    block_hash: string,
    --no-save
    --node: string = $DEFAULT_IP
] nothing -> any {
    log debug $"get block ($block_hash) part of file ($file_hash) from peer ($peer_id_base_58)"
    let res = $"get-block-from/($peer_id_base_58)/($file_hash)/($block_hash)/(not $no_save)" | run-command $node
    if $no_save {
        $res | get block_data | bytes from_int
    }
}

def "slash replace" [] string -> string {
    $in | str replace --all '/' '%2F'
}

export def get-file [
    file_hash: string,
    output_filename: string,
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"Getting file ($file_hash)"
    $"get-file/($file_hash)/($output_filename)" | run-command $node

}

export def get-blocks-info-from [
    peer_id_base_58: string,
    file_hash: string,
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"Getting the list of blocks from ($peer_id_base_58) for file ($file_hash)"
    $"get-blocks-info-from/($peer_id_base_58)/($file_hash)" | run-command $node
}

export def get-block-list [
    file_hash: string,
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"Getting the list of blocks for file ($file_hash) from own node"
    $"get-block-list/($file_hash)" | run-command $node
}

export def node-info [
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"Getting the info from node ($node)"
    "node-info" | run-command $node
}

export def send-block-list [
    file_hash: string,
    block_list: list<string>,
    --strategy-name: string = "RoundRobin"
    --node: string = $DEFAULT_IP,
] nothing -> any {
    log debug $"Sending the list of blocks ($block_list) from file ($file_hash) using the strategy ($strategy_name)"
    $"send-block-list" | run-command $node --post-body [$strategy_name, $file_hash, $block_list]
}

export def send-block-to [
    peer_id_base_58: string,
    file_hash: string,
    block_hash: string,
    --node: string = $DEFAULT_IP
] nothing -> any {
    log debug $"Sending block ($block_hash) part of file ($file_hash) to ($peer_id_base_58)"
    $"send-block-to" | run-command $node --post-body [$peer_id_base_58, $file_hash, $block_hash]
}

export def get-available-send-storage [
    --node: string = $DEFAULT_IP
] nothing -> any {
    log debug $"Getting the size left available for sending blocks from ($node)"
    $"get-available-send-storage" | run-command $node
}

# TODO: fix this, an integer is not accepted as data for the post body of Nushell's http post, but using a string doesn't allow parsing the int from the string on Rust's side
export def change-available-send-storage [
    --node: string = $DEFAULT_IP,
    new_storage_space: int,
] nothing -> any {
    log debug $"Changing the total available storage space to be ($new_storage_space)"
    $"change-available-send-storage" | run-command $node --post-body $new_storage_space
}
