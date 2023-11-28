use std log

while (
    swarm log | where msg == "Starting Dragoon Network" | get id
) != ($SWARM.seed) {
    log warning "swarm not up and running: waiting..."
    sleep 500ms
}

source listen.nu
source nodes.nu
source bootstrap_chain_n.nu
