use ../cli/swarm.nu *
use ../cli/app.nu
use ../cli/network_builder.nu *
use std assert
use help_func/exit_func.nu exit_on_error

# define variables
let test_file: path = "tests/assets/dragoon_32/dragoon_32x32.png"
let res_filename = "reconstructed_file.png"
let dragoonfly_root = "~/.share/dragoonfly"

print $"Removing ($dragoonfly_root) if it was there from a previous test\n"
try { rm -r "~/.share/dragoonfly" }

# create the nodes
const connection_list = [
    [1], 
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
    let peer_id_1 = app node-info --node $SWARM.1.ip_port

    let number_of_fails = 5

    print "\nNode 0 sends the block 0 to node 1, 3 times at once"
    let result_list = 0..$number_of_fails | par-each { |index|
        print $"Send index ($index)..."
        try {
            let res = app send-block-to --node $SWARM.0.ip_port $peer_id_1 $file_hash ($block_hashes | get 0)
            if not $res {
                error make {msg: $"Failed sending block ($index): ($block_hashes | get $index)"}
            }
            return 0
        } catch { |e|
            assert equal $e.msg $"unexpected error from Dragoon: Got error from command `send-block-to`: The send block to PeerId\(\"($peer_id_1)\"\) for block ($block_hashes.0) is already being handled \(500\)"
            return 1
        }
    }
    print "Node 0 finished sending blocks to node 1\n"
    
    print "Killing the swarm"
    swarm kill --no-shell

    print $"Checking we failed to send the block exactly ($number_of_fails) times as it was already being sent"
    assert equal ($result_list | math sum) $number_of_fails

    print "\nChecking the block 0 that was sent against the original"
    let original_block_path = $"($dragoonfly_root)/($peer_id_0)/files/($file_hash)/blocks/($block_hashes | get 0)"
    let sent_block_path     = $"($dragoonfly_root)/($peer_id_1)/files/($file_hash)/blocks/($block_hashes | get 0)"
    let difference = {diff ($original_block_path | path expand) ($sent_block_path |path expand)} | exit_on_error | get stdout
    if $difference != "" {
        print $"test failed, there was a difference between the blocks on index 0: ($block_hashes | get 0)"
        error make {msg: "Exit to catch"}
    }

    print $"(ansi light_green_reverse)    TEST SUCCESSFUL !(ansi reset)\n"

} catch { |e|
    print "Killing the swarm"
    swarm kill --no-shell
    error make --unspanned {msg: $"Test failed: ($e.msg)"}
}
