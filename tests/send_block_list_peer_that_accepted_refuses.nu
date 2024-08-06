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
    [1, 2], 
    [0],
    [0],
    ]

# create the network topology
let SWARM = build_network --no-shell $connection_list --storage_space [20, 20, 1] --unit_list [G, G, K]

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

    print "\nNode 0 sends the blocks to node 1 and 2"
    let distribution_list = app send-block-list --node $SWARM.0.ip_port --strategy_name "RoundRobin" $file_hash $block_hashes
    print "Node 0 finished sending blocks\n"
    print ($distribution_list | table --expand)

    let peer_id_2 = app node-info --node $SWARM.2.ip_port
    mut number_of_blocks_on_node_2 = 0
    for send_id in $distribution_list {
        if $send_id.0 == $peer_id_2 {
            $number_of_blocks_on_node_2 += 1
        }
    }
    if $number_of_blocks_on_node_2 != 1 {
        error make --unspanned {msg: $"Expected node 2 to have 1 block but it has ($number_of_blocks_on_node_2)"}
    }

    # Cannot check the distribution like we usually do in the other tests, because the node 2 receives 3 request of block storage at the same time
    # The one it accepts depends on what happens at runtime and we can't know that for sure

    print "\nChecking all the blocks that were sent against the original"
    for send_id in $distribution_list {
        let block_hash = $send_id.2
        let original_block_path = $"($dragoonfly_root)/($peer_id_0)/files/($file_hash)/blocks/($block_hash)"
        let peer_receiving_block = $send_id.0
        let sent_block_path     = $"($dragoonfly_root)/($peer_receiving_block)/files/($file_hash)/blocks/($block_hash)"

        let difference = {diff ($original_block_path | path expand) ($sent_block_path | path expand)} | exit_on_error | get stdout
        if $difference != "" {
            error make --unspanned {msg: $"there was a difference between the blocks ($block_hash) between peer ($peer_id_0) and ($peer_receiving_block)"}
        }
    }

    print "Killing the swarm"
    swarm kill --no-shell

} catch { |e|
    print "Killing the swarm"
    swarm kill --no-shell
    error make --unspanned {msg: $"Test failed: ($e.msg)"}
}