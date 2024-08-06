use ../cli/swarm.nu *
use ../cli/app.nu
use ../cli/network_builder.nu *
use std assert
use ../help_func/exit_func.nu exit_on_error
use ../help_func/get_remote.nu get_ssh_remote


def main [--ssh_addr_file: path] {

    # define variables
    let remote_output_path = "/tmp/dragoon_test"
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
    let SWARM = build_network --no-shell --replace_file_dir $connection_list --ssh_addr_file=$ssh_addr_file --storage_space [20, 20, 1] --unit_list [G, G, K]

    # remove previous output directory to ensure a fresh environment test
    for index in 0..(($SWARM | length) - 1) {
        if ($SWARM | get $index | get user) != "local" {
            try {
                let remote = get_ssh_remote $SWARM  $index
                ^ssh $remote $"rm -r ($remote_output_path)"
            }
        }
    }
    
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
        
        let peer_id_table = 0..(($connection_list | length) - 1) | each { |index|
            {(app node-info --node ($SWARM | get $index | get ip_port)) : $index}
        } | into record | flatten
        print $peer_id_table

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

        # check that the number of blocks per node is correct at least
        print "\nChecking all the blocks that were sent against the original"
        let block_node_index = $distribution_list | par-each { |send_id|
            let block_hash = $send_id.2
            let original_block_path = $"($dragoonfly_root)/($peer_id_0)/files/($file_hash)/blocks/($block_hash)"
            let peer_id_receiving_block = $send_id.0
            let sent_block_path     = $"($dragoonfly_root)/($peer_id_receiving_block)/files/($file_hash)/blocks/($block_hash)"
            let node_index = $peer_id_table | get $peer_id_receiving_block | get 0
            
            let difference = {
                if ($SWARM | get $node_index | get user) == "local" {
                    diff ($original_block_path | path expand) ($sent_block_path | path expand)
                } else {
                    let pre_cmd = $"mkdir -p ($remote_output_path) && rsync"
                    let remote = get_ssh_remote $SWARM $node_index
                    ^rsync -a --rsync-path $pre_cmd $original_block_path $"($remote):($remote_output_path)"
                    ^ssh $remote $"diff ($sent_block_path) ($remote_output_path)/($block_hash)"
                }
            } | exit_on_error | get stdout
            if $difference != "" {
                error make --unspanned {msg: $"there was a difference between the blocks ($block_hash) between peer ($peer_id_0) and ($peer_id_receiving_block)"}
            }
            $node_index
        }
                
        let acc = $block_node_index | each { |node_index|
            if $node_index == 1 {
                1
            } else if $node_index == 2 {
                0
            } else {
                error make {msg: $"Unexpected node index for block location: ($node_index)"}
            }   
        }
        
        assert equal ($acc | math sum) 4

        print "Killing the swarm"
        swarm kill --no-shell $SWARM

    } catch { |e|
        print "Killing the swarm"
        swarm kill --no-shell $SWARM
        error make {msg: $"Test failed: ($e)"}
    }
}
