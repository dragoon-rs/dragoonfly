use std log

log info "dialing in chain"
for pair in ($NODES | window 2) {
    app dial (app get-listeners --node $pair.0).0 --node $pair.1
}

log info "show connected peers in order"
for node in $NODES {
    print (app get-connected-peers --node $node)
}

log info "sleeping..."
sleep 500ms

log info "bootstrapping nodes"
for node in $NODES {
    app bootstrap --node $node
    sleep 200ms
    app bootstrap --node $node
}

log info "show connected peers in order"
for node in $NODES {
    print (app get-connected-peers --node $node)
}

app start-provide "foo" --node ($NODES | first)
app start-provide "bar" --node ($NODES | last)

print (app get-providers "foo" --node ($NODES | last))
print (app get-providers "bar" --node ($NODES | first))