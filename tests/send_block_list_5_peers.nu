use ../cli/swarm.nu *
use ../cli/dragoon.nu
use ../cli/network_builder.nu *
use std assert
use ../help_func/exit_func.nu exit_on_error
use ../help_func/get_remote.nu get_ssh_remote


def main [--ssh-addr-file: path] {

    # define variables
    let remote_output_path = "/tmp/dragoon_test"
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
    let SWARM = build_network --no-shell --replace-file-dir $connection_list --ssh-addr-file=$ssh_addr_file

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
        let encode_res = dragoon encode-file --node $SWARM.0.ip_port $test_file
        let block_hashes = $encode_res.1 | from json  #! This is a string not a list, need to convert
        let file_hash = $encode_res.0

        print $"The file got cut into blocks, block hashes are"
        print $block_hashes
        print $"The hash of the file is: ($file_hash)"

        print "\nGetting the peer id of the nodes"
        let peer_id_0 = dragoon node-info --node $SWARM.0.ip_port | get 0

        print "\nNode 0 sends the blocks to node 1, 2, 3, 4"
        let distribution_list = dragoon send-block-list --node $SWARM.0.ip_port --strategy-name "RoundRobin" $file_hash $block_hashes
        print "Node 0 finished sending blocks\n"
        print ($distribution_list | table --expand)

        # The index is the number of the block, the number is the node number that received the block `index`
        let expected_block_distribution = [1, 2, 3, 4, 1]

        let peer_id_list = 0..(($connection_list | length) - 1) | each {|index|
            {name: (dragoon node-info --node ($SWARM | get $index | get ip_port) | get 0), index: $index}
        }
        let peer_id_list = $peer_id_list | sort-by name

        print $peer_id_list

        print "\nChecking all the blocks that were sent against the original"
        0..(($block_hashes | length) - 1) | par-each {|index|
            let original_block_path = $"($dragoonfly_root)/($peer_id_0)/files/($file_hash)/blocks/($block_hashes | get $index)"
            let node_index = $expected_block_distribution | get $index
            let peer_receiving_block = ($peer_id_list | get $node_index | get name)
            let sent_block_path     = $"($dragoonfly_root)/($peer_receiving_block)/files/($file_hash)/blocks/($block_hashes | get $index)"

            let difference = {
                if ($SWARM | get $node_index | get user) == "local" {
                    diff ($original_block_path | path expand) ($sent_block_path | path expand)
                } else {
                    let pre_cmd = $"mkdir -p ($remote_output_path) && rsync"
                    let remote = get_ssh_remote $SWARM ($peer_id_list | get $node_index | get index)
                    ^rsync -a --rsync-path $pre_cmd $original_block_path $"($remote):($remote_output_path)"
                    ^ssh $remote $"diff ($sent_block_path) ($remote_output_path)/($block_hashes | get $index)"
                }
            } | exit_on_error | get stdout
            if $difference != "" {
                print $"test failed, there was a difference between the blocks on index ($index): ($block_hashes | get $index)"
                error make {msg: "Exit to catch"}
            }
        }

        print "Killing the swarm"
        swarm kill --no-shell $SWARM

    } catch { |e|
        print "Killing the swarm"
        swarm kill --no-shell $SWARM
        error make --unspanned {msg: $"Test failed: ($e)"}
    }
}