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
        [1], 
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
        let peer_id_1 = dragoon node-info --node $SWARM.1.ip_port | get 0

        print "\nGetting available storage size"
        let original_storage_space = dragoon get-available-send-storage --node $SWARM.1.ip_port

        print "\nNode 0 sends the blocks to node 1"
        0..(($block_hashes | length) - 1) | par-each { |index|
            print $"Sending block ($index)..."
            let res = dragoon send-block-to --node $SWARM.0.ip_port $peer_id_1 $file_hash ($block_hashes | get $index)
            if not $res.0 {
            error make {msg: $"Failed sending block ($index): ($block_hashes | get $index)"}
            }
        }
        print "Node 0 finished sending blocks to node 1\n"

        print "Checking that the reported available size makes sense with respect to the size of the blocks that were sent"
        let new_storage_space = dragoon get-available-send-storage --node $SWARM.1.ip_port
        let path = $"($dragoonfly_root)/($peer_id_0)/files/($file_hash)/blocks/"
        let size_of_all_sent_blocks = ls $path | get size | math sum | into int
        assert equal ($original_storage_space - $new_storage_space) $size_of_all_sent_blocks
        
        print "Killing the swarm"
        swarm kill --no-shell $SWARM

        print "\nChecking all the blocks that were sent against the original"
        let _ = 0..(($block_hashes | length) - 1) | par-each {|index|
            let original_block_path = $"($dragoonfly_root)/($peer_id_0)/files/($file_hash)/blocks/($block_hashes | get $index)"
            let sent_block_path     = $"($dragoonfly_root)/($peer_id_1)/files/($file_hash)/blocks/($block_hashes | get $index)"

            let difference = {
                if ($SWARM.1.user) == "local" {
                    diff ($original_block_path | path expand) ($sent_block_path | path expand)
                } else {
                    let pre_cmd = $"mkdir -p ($remote_output_path) && rsync"
                    let remote = get_ssh_remote $SWARM 1
                    ^rsync -a --rsync-path $pre_cmd $original_block_path $"($remote):($remote_output_path)"
                    ^ssh $remote $"diff ($sent_block_path) ($remote_output_path)/($block_hashes | get $index)"
                }
            } | exit_on_error | get stdout
            if $difference != "" {
                print $"test failed, there was a difference between the blocks on index ($index): ($block_hashes | get $index)"
                error make {msg: "Exit to catch"}
            }
        }

    } catch { |e|
        print "Killing the swarm"
        swarm kill --no-shell $SWARM
        error make --unspanned {msg: $"Test failed: ($e)"}
    }
}
