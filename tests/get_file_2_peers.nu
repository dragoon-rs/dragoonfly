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

        print "\nNode 0 starts providing the file"
        dragoon start-provide --node $SWARM.0.ip_port $file_hash

        let output_path = dragoon get-file --node $SWARM.1.ip_port $file_hash $res_filename
        print $"Output path for the file is ($output_path)"

        print "Killing the swarm"
        swarm kill --no-shell $SWARM

        if $SWARM.1.user != "local" {
            let pre_cmd = $"mkdir -p ($remote_output_path) && rsync"
            let remote = get_ssh_remote $SWARM 1
            ^rsync -a --rsync-path $pre_cmd $test_file $"($remote):($remote_output_path)"
        }

        print "Checking the difference between the original and reconstructed file"
        let difference = {
            if $SWARM.1.user == "local" {
                diff $output_path $test_file
            } else {
                let remote = get_ssh_remote $SWARM 1
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


