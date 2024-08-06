use ../cli/swarm.nu *
use ../cli/app.nu
use ../cli/network_builder.nu *
use std assert
use help_func/exit_func.nu exit_on_error

# define variables
let test_file: path = "tests/assets/dragoon_32/dragoon_32x32.png"
let res_filename = "reconstructed_file.png"
let dragoonfly_root = "~/.share/dragoonfly" | path expand

print $"Removing ($dragoonfly_root) if it was there from a previous test\n"
try { rm -r $dragoonfly_root }

# create the nodes
const connection_list = [
    [1, 2, 3, 4], 
    [0],
    [0],
    [0],
    [0],
    ]

# create the network topology
let SWARM = build_network --no-shell $connection_list

try {
    # Encode the file into blocks, put them to a directory named blocks next to the file
    print "Node 0 encodes the file into blocks"
    let encode_res = app encode-file --node $SWARM.0.ip_port $test_file
    let block_hashes = $encode_res.1 | from json  #! This is a string not a list, need to convert
    let file_hash = $encode_res.0

    print $"The file got cut into blocks, block hashes are"
    print $block_hashes
    print $"The hash of the file is: ($file_hash)"

    print "\nGetting the peer id of the nodes"
    let peer_id_0 = app node-info --node $SWARM.0.ip_port

    print "\nGetting available storage size"
    let original_storage_space = app get-available-storage --node $SWARM.1.ip_port

    print "\nNode 0 sends the blocks to node 1, 2, 3, 4"
    app send-block-list --node $SWARM.0.ip_port --strategy_name "RoundRobin" $file_hash $block_hashes
    print "Node 0 finished sending blocks\n"

    # The index is the number of the block, the number is the node number that received the block `index`
    let expected_block_distribution = [1, 2, 3, 4, 1]

    mut peer_id_list = []
    for index in 0..(($connection_list | length) - 1) {
        $peer_id_list = ($peer_id_list | append (app node-info --node ($SWARM | get $index | get ip_port)))
    }
    let peer_id_list = $peer_id_list | sort

    print "\nChecking all the blocks that were sent against the original"
    for index in 0..(($block_hashes | length) - 1) {
        let original_block_path = $"($dragoonfly_root)/($peer_id_0)/files/($file_hash)/blocks/($block_hashes | get $index)"
        let peer_receiving_block = ($peer_id_list | get ($expected_block_distribution | get $index))
        let sent_block_path     = $"($dragoonfly_root)/($peer_receiving_block)/files/($file_hash)/blocks/($block_hashes | get $index)"

        let difference = {diff ($original_block_path | path expand) ($sent_block_path | path expand)} | exit_on_error | get stdout
        if $difference != "" {
            print $"test failed, there was a difference between the blocks on index ($index): ($block_hashes | get $index)"
            error make {msg: "Exit to catch"}
        }
    }

    print "Killing the swarm"
    swarm kill --no-shell

} catch { |e|
    print "Killing the swarm"
    swarm kill --no-shell
    error make --unspanned {msg: $"Test failed: ($e.msg)"}
}