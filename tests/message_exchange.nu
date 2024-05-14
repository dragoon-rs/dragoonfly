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
let res_filename = "reconstructed_file.png"

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
    let encode_res = app encode-file --node $SWARM.0.ip_port $test_file
    let block_hashes = $encode_res.1 | from json  #! This is a string not a list, need to convert
    let file_hash = $encode_res.0

    print $"The file got cut into blocks, block hashes are: ($block_hashes)"
    print $"The hash of the file is: ($file_hash)"

    print "\nNode 0 starts providing the blocks"
    for $hash in $block_hashes {
        print $"Node 0 starts providing the block with hash ($hash)"
        app start-provide --node $SWARM.0.ip_port $hash
    }
    print "Node 0 finished announcing which blocks it provides\n"

    print "Node 1 starts searching for the providers with the hashes of the blocks"
    mut providers = []
    for $hash in $block_hashes {
        print $"Node 1 searching for the provider of the block with hash ($hash)"
        $providers = ($providers | append (app get-providers --node $SWARM.1.ip_port $hash | get 0))
    }
    print "Node 1 finished searching for the providers\n"
    print $"The providers are:"
    print $providers

    print "Creating the directory to receive the files"
    mkdir $output_dir

    print "\nNode 1 asks for the blocks to node 0"
    for $i in 0..<($block_hashes | length) {
        let $hash = $block_hashes | get $i
        print $"Getting block ($hash)"
        app get-block-from --node $SWARM.1.ip_port ($providers | get $i) $file_hash ($hash) | save $"($output_dir)/($hash)"
    }
    print "Finished getting all the blocks\n"
    
    print "Node 1 reconstructs the file with the blocks"
    app decode-blocks --node $SWARM.1.ip_port $output_dir $block_hashes $res_filename

    print "Killing the swarm"
    swarm kill --no-shell

    print "Checking the difference between the original and reconstructed file"
    let difference = diff $"($output_dir)/../($res_filename)" $test_file
    if $difference == "" {
        print "Test successful !"
    } else {
        print "test failed, there was a difference between the files"
        error make {msg: "Exit to catch"}
    }

} catch {
    print "Killing the swarm"
    swarm kill --no-shell
    error make {msg: "Test failed"}
}
