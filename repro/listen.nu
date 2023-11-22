use std log

log info "listening..."
for node in $SWARM {
    app listen $node.multiaddr --node $node.ip_port
    print (app get-peer-id --node $node.ip_port)
}
