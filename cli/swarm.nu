use std log
use std repeat

const NAME = "dragoonfly"
const LOG_DIR = ($nu.temp-path | path join $NAME)
const POWERS_PATH = "setup/powers/powers_test_Fr_155kB"

# create a swarm table
export def "swarm create" [
    n: int,
    --ssh_addr_file: path,
    --storage_space: list<int>,
    --unit_list: list<string>,
    ]: nothing -> table {
    if $storage_space != null {
        if ($storage_space | length) != $n {
            error make --unspanned {
                msg: "If a list of storage space is provided, it should be the same size as the number of nodes"
            }
        }
        if $unit_list != null {
            if ($unit_list | length) != $n {
                error make --unspanned {
                    msg: "If a list of unit is provided, it should be the same size as the number of nodes"
                }
            }
        }
        
    }

    let storage_space = match $storage_space {
        null => (20 | repeat $n),
        _ => $storage_space,
    }
    let unit_list = match $unit_list {
        null => ("G" | repeat $n),
        _ => $unit_list,
    }

    let addr_list = match $ssh_addr_file {
        null => ({user: "local", ip: "127.0.0.1"} | repeat $n),
        _ => {cat $ssh_addr_file | lines | skip 1 | take $n | parse "{user},{ip}"},
    }

    if ($addr_list | length) != $n {
        error make {
            msg: $"Tried to create a network with ($n) nodes but the list of addr only had ($addr_list | length) entries: ($addr_list)"
        }
    }
    
    seq 0 ($n - 1) | each { |index| {
        user: ($addr_list | get $index | get user),
        ip_port: $"($addr_list | get $index | get ip):(3_000 + $index)",
        seed: $index,
        multiaddr: $"/ip4/($addr_list | get $index | get ip)/tcp/(31_200 + $index)",
        storage: ($storage_space | get $index)
        unit: ($unit_list | get $index)
    } }
}

# run a swarm
export def "swarm run" [
    swarm: table<user: string, ip_port: string, seed: int, multiaddr: string, storage: int>, # the table of nodes to run
    --no_compile,
    --replace_file_dir,
    --features: list<string> = [], # features to include in the nodes
    --no-shell
]: nothing -> string {
    if ($swarm | is-empty) {
        error make --unspanned {
            msg: "`swarm create` requires a non empty swarm"
        }
    }
    if ($swarm.0.user != "local") {
        error make --unspanned {
            msg: "The first node should be spawned locally"
        }
    }

    let log_dir = $LOG_DIR | path join (random uuid)

    log info $"logging to `($log_dir)/*.log`"
    mkdir $log_dir

    if not $no_compile {
        ^cargo build --release --features ($features | str join ",") 
    }
    
    for node in $swarm {
        log info $"launching node ($node.seed) \(($node.ip_port)\)"
        let options = ( $" --ip-port ($node.ip_port)"
                + $" --seed ($node.seed)"
                + $" --storage-space ($node.storage)"
                + $" --storage-unit ($node.unit)"
                + (
                    if $replace_file_dir {
                        " --replace-file-dir"
                    } else {
                        ""
                    }
                )
                )

        let redirect = $"1> ($log_dir)/($node.seed).log 2> /dev/null &"

        if ($node.user == "local") {
            if not $no_compile {
                # FIXME: don't use Bash here
                ^bash -c (
                    $"cargo run --features '($features | str join ',')' -- --powers-path ($POWERS_PATH) ($options) ($redirect)"
                )
            } else {
                ^bash -c (
                    $"target/release/dragoonfly --powers-path ($POWERS_PATH) ($options) ($redirect)"
                )
            }
                
        } else {
            let ip = ($node.ip_port | parse "{ip}:{port}" | into record | get ip)
            let remote = $"($node.user)@($ip)"
            let target_path = "/tmp/target/release"
            let pre_cmd = $"mkdir -p ($target_path) && rsync"
            # copy the executable and the powers file to the remote
            # using rsync to not copy the file if it already exists
            ^rsync -a --rsync-path $pre_cmd target/release/dragoonfly $POWERS_PATH $"($remote):($target_path)"
            # launch node on the remote
            ^bash -c (
                $"ssh ($remote) '($target_path)/dragoonfly --powers-path ($target_path)/($POWERS_PATH | path basename ) ($options)' ($redirect)"
            )
            
        }
        
    }

    if not $no_shell {
        ^$nu.current-exe --execute $'
       $env.PROMPT_COMMAND = "SWARM-CONTROL-PANEL"
       $env.NU_LOG_LEVEL = "DEBUG"
       $env.SWARM_LOG_DIR = ($log_dir)
       use cli/app.nu
       use cli/swarm.nu ["swarm kill", "swarm list", "swarm log", "bytes decode"]
       const SWARM = ($swarm | to nuon)
    '
    }

    $log_dir
}

# list the nodes of the swarm
export def "swarm list" []: nothing -> table {
    ps | where name =~ $NAME
}

def parse-tracing-logs []: string -> table<date: datetime, level: string, id: int, file: string, msg: string> {
     lines
        | ansi strip
        | parse --regex '^(?<date>.{27}) (?<level>.{5}) (?<file>[\w:_-]*): (?<msg>.*)'
        | str trim level
        | into datetime date
}

export def "swarm log" []: nothing -> table<date: datetime, level: string, id: int, file: string, msg: string> {
    # FIXME: this should not require `mut`
    # related to https://github.com/nushell/nushell/issues/10428
    mut logs = []
    for id in (seq 0 (swarm list | length | $in - 1)) {
        let log_file = $env.SWARM_LOG_DIR | path join $"($id).log"
        if not ($log_file | path exists) {
            log warning $"`($log_file)` does not exist"
            continue
        }

        let log = open $log_file --raw | parse-tracing-logs | insert id $id | move id --before file
        $logs = ($logs | append $log)
    }

    let logs = $logs | sort-by date

    if not ($logs | is-empty) {
        let start = $logs.0.date
        $logs | update date { $in - $start }
    } else {
        []
    }
}

# kill the swarm
export def "swarm kill" [
    swarm: table<user: string, ip_port: string, seed: int, multiaddr: string, storage: int>
    --no-shell
    ]: nothing -> nothing {
    # kills all local node process
    ps | where name =~ $NAME | each {|it|
        log warning $"killing ($it.pid)"
        kill $it.pid
    }
    # kills all remote node process
    $swarm | filter {|node| $node.user != "local"} | each {|node|
        let ip = ($node.ip_port | parse "{ip}:{port}" | into record | get ip)
        let remote = $"($node.user)@($ip)"
        ^ssh $remote "ps -ef | grep 'dragoonfly' | grep -v grep | grep -v nu | awk '{print $2}' | xargs -r kill -15"
    }
    if not $no_shell {
        exit
    }
}

# decode a list of integer bytes into the underlying encoded string
export def "bytes decode" [encoding: string = "utf-8"]: list<int> -> string {
    each { into binary | bytes at 0..1 } | bytes collect | decode $encoding
}

# encode an encoded string into the underlying list of integer bytes
export def "bytes encode" [encoding: string = "utf-8"]: string -> list<int> {
    let bytes = $in | encode $encoding
    seq 1 ($bytes | bytes length) | each {|i|
        $bytes | bytes at ($i - 1)..($i) | into int
    }
}
