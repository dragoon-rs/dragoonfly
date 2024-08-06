
def main [--ssh_addr_file: path] {
    if $ssh_addr_file != null {
        print "Starting tests with ssh using the followings:"
        cat $ssh_addr_file
    }
    let all_tests = (ls tests/*.nu | where type == file)
    let total_number_of_tests = $all_tests | length 
    let error_list =  $all_tests | each { |test|
        let test_name =  ($test | get name)
        print $"\n(ansi yellow_reverse)    LAUNCHING TEST ($test_name)(ansi reset)\n"

        (if $ssh_addr_file == null {
            nu $test_name
        } else {
            nu $test_name --ssh_addr_file $ssh_addr_file
        }) e>| do { |e|
        let maybe_error = ($e | parse -r "(Error: .*)")
            if ($maybe_error | is-empty) {
                print $"(ansi light_green_reverse)    TEST SUCCESSFUL !(ansi reset)\n"
            } else {
                print $"(ansi red_reverse)    TEST FAILED(ansi reset)\n"
                {failed_test_name: $test_name, error: $maybe_error.0.capture0}
            }
        } $in
            
    } | compact

    print $"Total number of tests: ($total_number_of_tests)"

    if not ($error_list | is-empty) {
        print $"(ansi red_reverse)    ONE OR MORE TESTS FAILED(ansi reset)\n"
        print $error_list
        error make --unspanned {
            msg: "One or more tests failed" 
        }
    }
}
