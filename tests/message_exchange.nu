use ../swarm.nu *
use ../app.nu
use std assert

# define variables
let SWARM = swarm create 2
let record_name = "tata"
let message = "it works at least"
let sleep_duration = 5sec
mut node_is_setup = false
let output_dir: path = "/tmp/dragoon_test/received_blocks"
let test_file: path = "tests/assets/dragoon_32/dragoon_32x32.png"
let res_filename = "reconstructed_file"

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

    # a short sleep to ensure everything is setup properly
    print $"Sleeping for ($sleep_duration)"
    sleep $sleep_duration

    # Encode the file into blocks, put them to a directory named blocks next to the file
    print "Node 0 encodes the file into blocks"
    let result_string = app encode-file --node $SWARM.0.ip_port $test_file #! This is a string not a list, need to convert
    print $result_string
    let block_hashes = $result_string | from json

    print $"The file got cut into blocks, block hashes are: ($block_hashes)"

    print "\nNode 0 starts putting blocks into records"
    for $hash in $block_hashes {
        print $"Node 0 provides a record for the block with hash ($hash)"
        app put-record --node $SWARM.0.ip_port $hash $"($test_file | path dirname)/blocks"
    }
    print "Node 0 finished putting blocks into records\n"

    print "Node 1 starts searching for the blocks with the hashes"
    for $hash in $block_hashes {
        sleep 5sec
        print $"Node 1 getting the record for block with hash ($hash)"
        app get-record --node $SWARM.1.ip_port --output $"($output_dir)/($hash)" $hash
    }
    print "Node 1 finished searching for the blocks\n"

    print "Node 1 reconstructs the file with the blocks"
    app decode-blocks --node $SWARM.1.ip_port $output_dir $block_hashes $res_filename

    print "Killing the swarm"
    swarm kill --no-shell

    let difference = diff $"($output_dir)/($res_filename)" $test_file
    assert equal $difference ""

} catch {
    print "Killing the swarm"
    swarm kill --no-shell
    error make {msg: "Test failed"}
}
