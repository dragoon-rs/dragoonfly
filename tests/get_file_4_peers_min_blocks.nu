use ../cli/swarm.nu *
use ../cli/dragoon.nu
use ../cli/network_builder.nu *
use std assert
use ../help_func/exit_func.nu exit_on_error
use ../help_func/get_remote.nu get_ssh_remote

## This test will spawn 4 nodes, 3 of them will have exactly one block each and the fourth one will get the file
## This is the minimal number of blocks required to reconstruct the file
## The configuration of the nodes is as follows:
##          
## one block     
##      2 -- 3 get-file (needs 3 blocks)
##      |    |
##      0 -- 1 one block
##  one block

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
        [1, 2], 
        [3],
        [3],
        [1, 2]
        ]

    # create the network topology
    let SWARM = build_network --no-shell --replace-file-dir $connection_list --ssh-addr-file=$ssh_addr_file

    # remove previous output directory to ensure a fresh environment test
    # output directories are used when doing ssh and needing to compare the original block with the received block
    for index in 0..(($SWARM | length) - 1) {
        if ($SWARM | get $index | get user) != "local" {
            try {
                let remote = get_ssh_remote $SWARM  $index
                ^ssh $remote $"rm -r ($remote_output_path)"
            }
        }
    }

    try {

        # Encode the file into blocks
        print "Node 0 encodes the file into blocks"
        # encode-file has k=3 et n=5 default values, meaning 5 blocks are produced and 3 are needed to decode the file
        let encode_res = dragoon encode-file --node $SWARM.0.ip_port $test_file
        let block_hashes = $encode_res.1 | from json  #! This is a string not a list, need to convert
        let file_hash = $encode_res.0

        print $"The file got cut into blocks, block hashes are"
        print $block_hashes
        print $"The hash of the file is: ($file_hash)"

        # the peer id of the node 0
        let peer_id_0 = dragoon node-info --node $SWARM.0.ip_port | get 0

        # Deleting two blocks (we only have 3 left, the minimal number required to decode the file)
        for i in 3..4 {
            print $"Removing blocks number ($i) from node 0: ($block_hashes | get $i)"
            rm $"($dragoonfly_root)/($peer_id_0)/files/($file_hash)/blocks/($block_hashes | get $i)"
        }
        print

        

        # moving blocks to other nodes
        # We move the block 1 to node 1 and the block 2 to node 2
        # To do this, we actually do a get-block-from from those nodes, requesting the block to node 0
        # In normal cases, node 1 and 2 wouldn't know about those blocks, they would need to first:
        #
        # Know the file hash
        # Ask for providers of this file hash (meaning node 0 should have used start provide)
        # Ask for the block list to the providers
        # Then finally do a get-block-from
        #
        # But we simplify here since in the test we already know those informations
        for i in 1..2 {
            print $"Node ($i) requests block ($block_hashes | get $i)"
            dragoon get-block-from --node ($SWARM | get $i | get ip_port) $peer_id_0 $file_hash ($block_hashes | get $i)
        }
        print "Finished transferring the blocks"

        # Node 0, 1 and 2 start providing the file
        for i in 0..2 {
            print $"\nNode ($i) starts providing the file"
            dragoon start-provide --node ($SWARM | get $i | get ip_port) $file_hash
        }

        # Node 3 gets the file
        let output_path = dragoon get-file --node $SWARM.3.ip_port $file_hash $res_filename
        print $"Output path for the file is ($output_path)"

        print "Killing the swarm"
        swarm kill --no-shell $SWARM

        # If using ssh, send the original file to the node 3 so it can compare between the decoded and the original
        if $SWARM.3.user != "local" {
            let pre_cmd = $"mkdir -p ($remote_output_path) && rsync"
            let remote = get_ssh_remote $SWARM 3
            ^rsync -a --rsync-path $pre_cmd $test_file $"($remote):($remote_output_path)"
        }

        # Check that files are identical
        print "Checking the difference between the original and reconstructed file"
        let difference = {
            if $SWARM.3.user == "local" {
                diff $output_path $test_file
            } else {
                let remote = get_ssh_remote $SWARM 3
                ^ssh $remote $"diff ($output_path) ($remote_output_path)/($test_file | path basename)"
            }
            
        } | exit_on_error | get stdout
        if $difference != "" {
            print "test failed, there was a difference between the files"
            error make {msg: "there was a difference between the files"}
        }

    } catch { |e|
        print "Killing the swarm"
        swarm kill --no-shell $SWARM
        error make --unspanned {msg: $"Test failed: ($e)"}
    }
}