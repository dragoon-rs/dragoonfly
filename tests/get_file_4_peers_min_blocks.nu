use ../cli/swarm.nu *
use ../cli/app.nu
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
        [3],
        [3],
        [1, 2]
        ]

    # create the network topology
    let SWARM = build_network --no-shell --replace_file_dir $connection_list --ssh_addr_file=$ssh_addr_file

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

        let peer_id_0 = app node-info --node $SWARM.0.ip_port

        for i in 3..4 {
            print $"Removing blocks number ($i) from node 0: ($block_hashes | get $i)"
            rm $"($dragoonfly_root)/($peer_id_0)/files/($file_hash)/blocks/($block_hashes | get $i)"
        }
        print

        

        # moving blocks to other nodes
        for i in 1..2 {
            print $"Node ($i) requests block ($block_hashes | get $i)"
            app get-block-from --node ($SWARM | get $i | get ip_port) $peer_id_0 $file_hash ($block_hashes | get $i)
        }
        print "Finished transferring the blocks"

        # First three nodes start providing the file
        for i in 0..2 {
            print $"\nNode ($i) starts providing the file"
            app start-provide --node ($SWARM | get $i | get ip_port) $file_hash
        }

        let output_path = app get-file --node $SWARM.3.ip_port $file_hash $res_filename
        print $"Output path for the file is ($output_path)"

        print "Killing the swarm"
        swarm kill --no-shell $SWARM

        if $SWARM.3.user != "local" {
            let pre_cmd = $"mkdir -p ($remote_output_path) && rsync"
            let remote = get_ssh_remote $SWARM 3
            ^rsync -a --rsync-path $pre_cmd $test_file $"($remote):($remote_output_path)"
        }

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
            error make {msg: "Exit to catch"}
        }

    } catch { |e|
        print "Killing the swarm"
        swarm kill --no-shell $SWARM
        error make --unspanned {msg: $"Test failed: ($e)"}
    }
}