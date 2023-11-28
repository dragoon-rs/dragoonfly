use std log

for node in $SWARM {
    app listen $node.multiaddr --node $node.ip_port

    let peer_id = app get-peer-id --node $node.ip_port
    let listener = app get-listeners --node $node.ip_port | get 0
    log info $"($peer_id) listening on ($listener)"
}
