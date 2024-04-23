use ../swarm.nu *
use ../app.nu
use std assert

# define variables
let SWARM = swarm create 2
let record_name = "tata"
let message = "it works at least"
let sleep_duration = 1sec
mut node_is_setup = false

# create the nodes
swarm run --no-shell $SWARM

try {
    print "Env var inside the script"
    try {
        print $'http_proxy: ($env.http_proxy)'
    } catch {
        print "http_proxy not set"
    }

    try { 
        print $'HTTP_PROXY: ($env.HTTP_PROXY)' 
    } catch {
        print "HTTP_PROXY not set"
    }

    let original_time = date now

    # make the node start to listen on their own ports
    print "Trying to listen with node 0"
    while not $node_is_setup {
        # try to listen on node 0, this is to allow time for the ports to setup properly
        try {
            sleep 1sec
            app listen --node $SWARM.0.ip_port $SWARM.0.multiaddr
            print "Exiting the try-listen"
            $node_is_setup = true
        } catch {
            let current_time = (date now)
            let elapsed_time = $current_time - $original_time
            print -n $"Failed to listen, elapsed_time: ($elapsed_time | format duration sec)\r"
        }
    }
    print $"Successfully listening on node 0, took: ((date now) - $original_time | format duration sec)"
    
    # Node 1 doesn't need the same kind of setup as node 0, since it's only needed for the first node to do this
    print "Trying to listen with node 1"
    app listen --node $SWARM.1.ip_port $SWARM.1.multiaddr
    print "Node 1 listening"

    # connect the nodes
    print "Node 0 dialing node 1"
    app dial --node $SWARM.0.ip_port $SWARM.1.multiaddr

    # announce that you provide a key and add a message with the given key
    print "Node 0 announces it has a given record"
    app start-provide --node $SWARM.0.ip_port $record_name
    # a short sleep to ensure everything is setup properly
    print $"Sleeping for ($sleep_duration)"
    sleep $sleep_duration
    print "Node 0 gives a value associated with the record key"
    app put-record --node $SWARM.0.ip_port $record_name $message

    # get the value associated to the key
    print "Node 1 searches for the value associated with the record key"
    let res = app get-record --node $SWARM.1.ip_port $record_name | bytes decode



    print "Killing the swarm"
    swarm kill --no-shell

    assert equal $res $message
} catch {
    print "Test failed"
    print "Killing the swarm"
    swarm kill --no-shell
}
