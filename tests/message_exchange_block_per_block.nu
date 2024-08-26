use ../cli/swarm.nu *
use ../cli/dragoon.nu
use ../cli/network_builder.nu *
use std assert
use ../help_func/exit_func.nu exit_on_error
use ../help_func/get_remote.nu get_ssh_remote

def main [--ssh-addr-file: path] {

    # define variables
    let output_dir: path = "/tmp/dragoon_test/received_blocks"
    let original_file: path = "tests/assets/dragoon_32/dragoon_32x32.png"
    let test_file: path = "/tmp/dragoon_test/test_file.png"
    let res_filename = "reconstructed_file.png"
    let dragoonfly_root = "~/.share/dragoonfly" | path expand

    print $"Removing ($dragoonfly_root) if it was there from a previous test\n"
    try { rm -r $dragoonfly_root }

    const connection_list = [
        [1], 
        [0],
        ]

    # create the network topology
    let SWARM = build_network --no-shell --replace-file-dir $connection_list --ssh-addr-file=$ssh_addr_file

    try {

        mkdir ($test_file | path dirname)
        cp $original_file $test_file

        if $SWARM.1.user != "local" {
            print "Sending the test file to node 1 through rsync"
            let pre_cmd = $"mkdir -p ($test_file | path dirname) && rsync"
            let remote = get_ssh_remote $SWARM 1
            ^rsync -a --rsync-path $pre_cmd $original_file $"($remote):($test_file)"

        }

        # Encode the file into blocks, put them to a directory named blocks next to the file
        print "Node 1 encodes the file into blocks"
        let encode_res = dragoon encode-file --node $SWARM.1.ip_port $test_file
        let block_hashes = $encode_res.1 | from json  #! This is a string not a list, need to convert
        let file_hash = $encode_res.0

        print $"The file got cut into blocks, block hashes are"
        print $block_hashes
        print $"The hash of the file is: ($file_hash)"

        print "\nNode 1 starts providing the file"
        dragoon start-provide --node $SWARM.1.ip_port $file_hash

        print "Node 0 starts searching for the providers with the file hash"
        let provider = dragoon get-providers --node $SWARM.0.ip_port $file_hash | get 0
        print $"The providers are:"
        print $provider

        print "\nNode 0 asks node 1 to provide the list of blocks it has for the file"
        let received_block_list = dragoon get-blocks-info-from --node $SWARM.0.ip_port $provider $file_hash | get block_hashes
        print $"The blocks node 1 has are:"
        print $received_block_list

        print "\nComparing to the actual block list:"
        assert equal ($block_hashes |sort) ($received_block_list |sort)
        print "Passed ! Block lists are the same"


        print "Creating the directory to receive the files"
        mkdir $output_dir

        print "\nNode 0 asks for the blocks to node 1"
        for $i in 0..<($received_block_list | length) {
            let $hash = $received_block_list | get $i
            print $"Getting block ($hash)"
            dragoon get-block-from --node $SWARM.0.ip_port --no-save $provider $file_hash ($hash) | save $"($output_dir)/($hash)"
        }
        print "Finished getting all the blocks\n"
        
        print "Node 0 reconstructs the file with the blocks"
        dragoon decode-blocks --node $SWARM.0.ip_port $output_dir $received_block_list $res_filename

        print "Killing the swarm"
        swarm kill --no-shell $SWARM

        print "Checking the difference between the original and reconstructed file"

        let difference = {diff $"($output_dir)/../($res_filename)" ($original_file)} | exit_on_error | get stdout
        if $difference != "" {
            print "test failed, there was a difference between the files"
            error make {msg: "Exit to catch"}
        }

    } catch { |e|
        print "Killing the swarm"
        swarm kill --no-shell $SWARM
        error make {msg: $"Test failed: ($e)"}
    }
}