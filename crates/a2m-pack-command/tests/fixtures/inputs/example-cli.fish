# Completions for example-cli, the imaginary inventory program of the
# command pack's test stand. Written for the tests: it describes nothing
# real. Some lines are in forms the reader does not take, on purpose.

complete -c example-cli -f
complete -c example-cli -n __fish_use_subcommand -a item -d 'Work with items'
complete -c example-cli -l home -r -d 'The folder that holds the data'

complete -c example-cli -n '__fish_seen_subcommand_from show' -l format -x -a 'text json' -d 'How to print the item'
complete -c example-cli -n '__fish_seen_subcommand_from item; and __fish_seen_subcommand_from set' -l yes -d 'Do not ask'
complete -c example-cli -n '__fish_seen_subcommand_from set' -l if-version -x -d 'Change the item only at this version'
complete -c example-cli -n '__fish_seen_subcommand_from push' -l dry-run -d 'Show what would be sent, and send nothing'
complete -c example-cli -n '__fish_seen_subcommand_from archive' -l all -d 'Every item'

# Worked out when fish completes: a program fish would run. Never read.
complete -c example-cli -n '__fish_seen_subcommand_from list' -l location -x -a '(example-cli catalog push --yes)'
complete -c example-cli -n "__fish_seen_subcommand_from $example_commands" -l verbose
complete -c example-cli -n '__example_cli_needs_item' -l quiet

function __example_cli_needs_item
    complete -c example-cli -l hidden
end
