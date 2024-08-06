use swarm.nu *
use app.nu
use std log

# Makes a network topology by connecting nodes according to the given network matrix
## Example
#
# [
#   [1, 4],
#   [0, 2],
#   [1, 3],
#   [2, 4],
#   [0, 3],
# ]
#
# Will connect node 0 to 1 and 4, node 1 to 2, node 2 to 3  and 3 to 4 (connecting only to nodes whose number is bigger than current node to prevent double dial)
#
# Basically doing the following:
#         3
#      /     \
#    4        2
#     \      /
#       0 - 1
## Note
#
# You do not need to write every connection, as connections go both way. This builder only makes the connection if the number of the node it connects to is bigger than its own number
# This means that the previous list could also be written:
#
# [
#   [1, 4],
#   [2],
#   [3],
#   [4],
#   [0],
# ]
#
# Note that because of type conversion, an empty list is not acceptable, so you can just write [0] instead of empty lists (will be ignored for all nodes), since 0 is the smallest node number
#
export def build_network [
    connection_list: list<list<int>>, 
    --no-shell,
    --no_compile,
    --replace_file_dir,
    --ssh_addr_file: path,
    --storage_space: list<int>,
    --unit_list: list<string>,
    ]: nothing -> table {
    let matrix_size = $connection_list | length

    print $"(ansi light_green_reverse)Launching the network(ansi reset)"
    let SWARM = swarm create $matrix_size --ssh_addr_file $ssh_addr_file --storage_space $storage_space --unit_list $unit_list
    mut run_options = ""
    let log_dir = swarm run --no-shell --no_compile=$no_compile  --replace_file_dir=$replace_file_dir $SWARM

    print $SWARM

    log debug "Env var inside the script"
    try {
        log debug $'http_proxy: ($env.http_proxy)'
    } catch {
        log debug "http_proxy not set"
    }

    try { 
        log debug $'HTTP_PROXY: ($env.HTTP_PROXY)' 
    } catch {
        log debug "HTTP_PROXY not set"
    }

    try {
        mut node_is_setup = false
        let original_time = date now

        # make the node start to listen on their own ports
        log debug "Trying to listen with node 0\n"
        while not $node_is_setup {
            # try to listen on node 0, this is to allow time for the ports to setup properly
            try {
                sleep 1sec
                app listen --node $SWARM.0.ip_port $SWARM.0.multiaddr
                log debug "\nExiting the try-listen for node 0 after success"
                $node_is_setup = true
            } catch {
                let current_time = (date now)
                let elapsed_time = $current_time - $original_time
                print -n $"Failed to listen on node 0, elapsed_time: ($elapsed_time | format duration sec)\r"
            }
        }

        log debug "Making all the other nodes start to listen on their server ports"
        #! this doesn't work with nushell 0.95
        1..($matrix_size - 1) | each { |i|
            log debug $"Trying to listen on ($i)"
            # ssh here to launch the listen from the node itself
            if (($SWARM | get $i | get user) == "local") {
                app listen --node ($SWARM | get $i | get ip_port) ($SWARM | get $i | get multiaddr)
            } else {
                let node = ($SWARM | get $i)
                let ip = ($node | get ip_port | parse "{ip}:{port}" | into record | get ip)
                let remote = $"($node | get user)@($ip)"
                let _ = (^ssh $remote $"curl "http://($node | get ip_port)/listen/($node | get multiaddr | str replace --all '/' '%2F')"" | complete)
            }
            
            log debug $"Successful listen on ($i)"
        }
        log info "Finished setting up the nodes for listen, starting the dials"

        log debug "Starting to dial the nodes"
        0..($matrix_size - 1) | each { |i|
            let connect_to = ($connection_list | get $i) | filter {|x| $x > $i} | each {|x| $SWARM | get $x | get multiaddr}
            #? do commands still work when using the --node like that
            app dial-multiple --node ($SWARM | get $i | get ip_port) $connect_to
        }

        log info "Finished dialing, launching console"

        if not $no_shell {
            ^$nu.current-exe --execute $'
            $env.PROMPT_COMMAND = "SWARM-CONTROL-PANEL"
            $env.NU_LOG_LEVEL = "DEBUG"
            $env.SWARM_LOG_DIR = ($log_dir)
            use cli/app.nu
            use cli/swarm.nu ["swarm kill", "swarm list", "swarm log", "bytes decode"]
            const SWARM = ($SWARM |to nuon)
            '
        } else {
            return $SWARM
        }
    } catch { |e|
        log info "Killing the swarm"
        swarm kill --no-shell $SWARM
        error make --unspanned {msg: $"Builder failed: ($e.msg)"}
    }
}