if ($NODES | length) != 4 {
    error make --unspanned {
        msg: $"expected 4 nodes in the network, found ($NODES | length)"
    }
}

for pair in ($NODES | window 2) {
    app dial (app get-listeners --node $pair.0).0 --node $pair.1
}

for node in $NODES {
    print (app get-connected-peers --node $node)
}

sleep 500ms

for node in $NODES {
    app bootstrap --node $node
}

for node in $NODES {
    print (app get-connected-peers --node $node)
}

app start-provide "foo" --node $NODES.0
print (app get-providers "foo" --node $NODES.3)

app start-provide "bar" --node $NODES.3
print (app get-providers "bar" --node $NODES.0)
