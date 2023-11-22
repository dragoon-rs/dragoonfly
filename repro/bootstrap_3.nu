if ($NODES | length) != 3 {
    error make --unspanned {
        msg: $"expected 3 nodes in the network, found ($NODES | length)"
    }
}

let bootstrap = (app get-listeners --node $NODES.0).0
app dial $bootstrap --node $NODES.1
app dial $bootstrap --node $NODES.2

for node in $NODES {
    print (app get-connected-peers --node $node)
}

sleep 500ms

app bootstrap --node $NODES.1
app bootstrap --node $NODES.2

for node in $NODES {
    print (app get-connected-peers --node $node)
}

app start-provide "foo" --node $NODES.1
print (app get-providers "foo" --node $NODES.2)

app start-provide "bar" --node $NODES.2
print (app get-providers "bar" --node $NODES.1)
