export def get_ssh_remote [
    swarm: table<user: string, ip_port: string, seed: int, multiaddr: string, storage: int>
    node_index: int,
    ] nothing -> string {
    let node = $swarm | get $node_index
    let ip = ($node.ip_port | parse "{ip}:{port}" | into record | get ip)
    $"($node.user)@($ip)"
}