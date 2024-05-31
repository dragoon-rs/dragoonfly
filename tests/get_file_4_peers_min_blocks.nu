use ../swarm.nu *
use ../app.nu
use std assert
use help_func/exit_func.nu exit_on_error

## This test will spawn 4 nodes, 3 of them will have exactly one block each and the fourth one will get the file
## This is the minimal number of blocks required to reconstruct the file
## The configuration of the nodes is as follows:
##          2 one block
##            \
##              3 get-file (needs 3 blocks)
##            /
##      0 -- 1 one block
##  one block

# define variables
let SWARM = swarm create 4
let sleep_duration = 5sec
mut node_is_setup = false
let test_file: path = "tests/assets/dragoon_32/dragoon_32x32.png"
let res_filename = "reconstructed_file.png"
let dragoonfly_root = "~/.share/dragoonfly"

print $"Removing ($dragoonfly_root) if it was there from a previous test\n"
try { rm -r $dragoonfly_root }

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
    for i in 1..3 {
        print $"\nTrying to listen with node ($i)"
        app listen --node ($SWARM | get $i | get ip_port) ($SWARM | get $i | get multiaddr)
        print $"Node ($i) listening"
    }

    # connect the nodes
    print "Node 0 dialing node 1"
    app dial --node $SWARM.0.ip_port $SWARM.1.multiaddr

    print "Node 1 dialing node 3"
    app dial --node $SWARM.1.ip_port $SWARM.3.multiaddr

    print "Node 2 dialing node 3"
    app dial --node $SWARM.2.ip_port $SWARM.3.multiaddr

    # a short sleep to ensure everything is setup properly
    print $"Sleeping for ($sleep_duration)"
    sleep $sleep_duration

    # Encode the file into blocks, put them to a directory named blocks next to the file
    print "Node 0 encodes the file into blocks"
    let encode_res = app encode-file --node $SWARM.0.ip_port $test_file
    let block_hashes = $encode_res.1 | from json  #! This is a string not a list, need to convert
    let file_hash = $encode_res.0

    print $"The file got cut into blocks, block hashes are"
    print $block_hashes
    print $"The hash of the file is: ($file_hash)"

    mut peer_id_list = []
    mut block_location_list = []
    for index in 0..3 {
        $peer_id_list = ($peer_id_list | append (app node-info --node ($SWARM | get $index | get ip_port)))
        $block_location_list = ($block_location_list |append $"($dragoonfly_root)/($peer_id_list | get $index)/files/($file_hash)/blocks")
    }

    print "\nPeer id of all the nodes:"
    print $peer_id_list
    print

    print "\nBlock location of all the nodes:"
    print $block_location_list
    print

    for i in 3..4 {
        print $"Removing blocks number ($i) from node 0: ($block_hashes | get $i)"
        rm $"($block_location_list.0)/($block_hashes | get $i)"
    }
    print

    # moving blocks to other nodes
    # note that this could also be done by a get-block-from
    for i in 1..2 {
        print $"Creating directory for the block ($i) at ($block_location_list | get $i)"
        mkdir ($block_location_list | get $i)
        print $"Moving block ($block_hashes | get $i) to node ($i)"
        mv $"($block_location_list.0)/($block_hashes | get $i)" $"($block_location_list | get $i)/"
    }

    # First three nodes start providing the file
    for i in 0..2 {
        print $"\nNode ($i) starts providing the file"
        app start-provide --node ($SWARM | get $i | get ip_port) $file_hash
    }

    print $"\nSleeping for ($sleep_duration) to ensure the start provide finished"
    sleep $sleep_duration
    print "Resuming execution\n"

    let output_path = app get-file --node $SWARM.3.ip_port $file_hash $res_filename
    print $"Output path for the file is ($output_path)"

    print "Killing the swarm"
    swarm kill --no-shell

    print "Checking the difference between the original and reconstructed file"
    let difference = {diff $output_path $test_file} | exit_on_error | get stdout
    if $difference == "" {
        print $"(ansi light_green_reverse)    TEST SUCCESSFUL !(ansi reset)\n"
    } else {
        print "test failed, there was a difference between the files"
        error make {msg: "Exit to catch"}
    }

} catch { |e|
    print "Killing the swarm"
    swarm kill --no-shell
    error make --unspanned {msg: $"Test failed: ($e.msg)"}
}
