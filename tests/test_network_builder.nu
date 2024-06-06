use ../cli/swarm.nu *
use ../cli/app.nu
use ../cli/network_builder.nu *
use std assert

const connection_list = [
    [1],
    [0, 3],
    [3],
    [1,2,4],
    [3,5,6],
    [4],
    [4],
    ]

let SWARM = build_network --no-shell $connection_list
try {
    let node_number = $SWARM | length
    assert equal $node_number ($connection_list | length)
    mut name_list: table = [{(app get-peer-id --node ($SWARM.0.ip_port)): 0 }]
    
    for i in 1..($node_number - 1) {
        $name_list = ($name_list | merge [{(app get-peer-id --node ($SWARM | get $i | get ip_port)): $i }])
    }
    print "Names of the nodes are:"
    print $name_list
    
    mut actual_connection_list: list<list<int>> = []

    for i in 0..($node_number - 1) {
        let node_peers = app get-connected-peers --node ($SWARM | get $i | get ip_port)
        mut corresponding_node_number = []
        for peer_id in $node_peers {
            let number = ($name_list | get $peer_id | get 0)
            $corresponding_node_number = ($corresponding_node_number | append $number)
        }
        
        $actual_connection_list = ($actual_connection_list | append [($corresponding_node_number | sort)])
    }

    print $"Expected connection list is:($connection_list)"

    print $"Actual connection list is: ($actual_connection_list)"

    assert equal $connection_list $actual_connection_list

    print "Killing the swarm"
    swarm kill --no-shell

    print $"(ansi light_green_reverse)    TEST SUCCESSFUL !(ansi reset)\n" 
} catch { |e|
    print "Killing the swarm"
    swarm kill --no-shell
    error make --unspanned {msg: $"Test failed: ($e.msg)"}
}

