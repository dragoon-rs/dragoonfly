use ../cli/swarm.nu *
use ../cli/app.nu
use ../cli/network_builder.nu *
use std assert
use ../help_func/exit_func.nu exit_on_error

# test is the same with our without ssh

def main [--ssh_addr_file: path] {

    # define variables
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
    let SWARM = build_network --no-shell --replace_file_dir $connection_list --ssh_addr_file=$ssh_addr_file --storage_space [20, 0]

    try {
        # Encode the file into blocks, put them to a directory named blocks next to the file
        print "Node 0 encodes the file into blocks"
        let encode_res = app encode-file --node $SWARM.0.ip_port $test_file
        let block_hashes = $encode_res.1 | from json  #! This is a string not a list, need to convert
        let file_hash = $encode_res.0

        print $"The file got cut into blocks, block hashes are"
        print $block_hashes
        print $"The hash of the file is: ($file_hash)"

        print "\nNode 0 sends the blocks to node 1"
        app send-block-list --node $SWARM.0.ip_port $file_hash $block_hashes
        print "Node 0 finished sending blocks to node 1\n"

        print "Killing the swarm"
        swarm kill --no-shell $SWARM

        error make --unspanned {msg: "Send block list should have returned an error because node 1 doesn't have enough storage space to accept blocks"}

    } catch { |e|
        print "Killing the swarm"
        swarm kill --no-shell $SWARM

        if ($e.msg | str contains 'SendBlockListFailed { final_block_distribution: [], context: "No more peers to send but blocks are left" }') {
            return # test successful, we got the error we expected
        } else {
            error make --unspanned {msg: $"Test failed: ($e)"}
        }
    }
}
