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

    print "\nNode 0 starts providing the file"
    app start-provide --node $SWARM.0.ip_port $file_hash

    let output_path = app get-file --node $SWARM.1.ip_port $file_hash $res_filename
    print $"Output path for the file is ($output_path)"

    print "Killing the swarm"
    swarm kill --no-shell

    print "Checking the difference between the original and reconstructed file"
    let difference = {diff $output_path $test_file} | exit_on_error | get stdout
    if $difference == "" {
        print $"(ansi light_green_reverse)    TEST SUCCESSFUL !(ansi reset)\n"
    } else {
        print "test failed, there was a difference between the files"
        error make {msg: "Exit to catch"}
    }

} catch { |e|
    print "Killing the swarm"
    swarm kill --no-shell
    error make --unspanned {msg: $"Test failed: ($e.msg)"}
}
