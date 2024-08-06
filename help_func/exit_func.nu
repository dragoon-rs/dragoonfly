export def exit_on_error []: closure -> record<stdout: string, stderr: string> {
    let res = do $in | complete
    if $res.exit_code != 0 {
        error make --unspanned { msg: $res.stderr }
    }

    $res | reject exit_code
}