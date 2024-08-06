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
    let SWARM = build_network --no-shell --replace_file_dir $connection_list --ssh_addr_file=$ssh_addr_file --storage_space [20, 20, 0]
    
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

        print "\nGetting available storage size"
        let original_storage_space = app get-available-storage --node $SWARM.1.ip_port

        print "\nNode 0 sends the blocks to node 1 and 2"
        let res = app send-block-list --node $SWARM.0.ip_port --strategy_name "RoundRobin" $file_hash $block_hashes
        print "Node 0 finished sending blocks\n"
        print ($res | table --expand)

        let peer_id_1 = app node-info --node $SWARM.1.ip_port

        print "\nChecking all the blocks that were sent against the original"
        0..(($block_hashes | length) - 1) | par-each {|index|
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

        print "Killing the swarm"
        swarm kill --no-shell $SWARM

    } catch { |e|
        print "Killing the swarm"
        swarm kill --no-shell $SWARM
        error make --unspanned {msg: $"Test failed: ($e)"}
    }
}
