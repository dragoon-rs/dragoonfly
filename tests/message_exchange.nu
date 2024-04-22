use ../swarm.nu *
use ../app.nu
use std assert

# define variables
let SWARM = swarm create 2
let record_name = "tata"
let message = "it works at least"

# create the nodes
swarm run --no-shell $SWARM

print "Sleeping for 1s to setup ports"
sleep 1sec
print "Resuming execution"

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

# make the node start to listen on their own ports
print "Node 0 listening"
app listen --node $SWARM.0.ip_port $SWARM.0.multiaddr
print "Node 1 listening"
app listen --node $SWARM.1.ip_port $SWARM.1.multiaddr

# connect the nodes
print "Node 0 dialing node 1"
app dial --node $SWARM.0.ip_port $SWARM.1.multiaddr

# announce that you provide a key and add a message with the given key
print "Node 0 announces it has a given record"
app start-provide --node $SWARM.0.ip_port $record_name
print "Sleeping for 1s"
sleep 1sec
print "Node 0 gives a value associated with the record key"
app put-record --node $SWARM.0.ip_port $record_name $message

# get the value associated to the key
print "Node 1 searches for the value associated with the record key"
let res = app get-record --node $SWARM.1.ip_port $record_name | bytes decode



print "Killing the swarm"
swarm kill --no-shell

assert equal $res $message