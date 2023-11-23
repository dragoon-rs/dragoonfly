use std log

if ($NODES | length) != 2 {
    error make --unspanned {
        msg: $"expected 2 nodes in the network, found ($NODES | length)"
    }
}

log info "dialing in chain"
app dial (app get-listeners --node $NODES.0).0 --node $NODES.1

log info "sleeping..."
sleep 500ms

log info "bootstrapping nodes"
app bootstrap --node $NODES.1
app bootstrap --node $NODES.0

app start-provide "foo" --node $NODES.0
app add-file "this is foo" --node $NODES.0
print (app get-providers "foo" --node $NODES.1)
print (app get "foo" --node $NODES.1)
