<#
  The manifest of the imaginary module Example.Inventory, written for the
  command pack's tests. It describes no real product.
#>
@{
    RootModule        = 'Example.Inventory.psm1'
    ModuleVersion     = '1.4.0'
    GUID              = '00000000-0000-0000-0000-000000000001'
    Author            = 'Example'
    Description       = 'Reads and sets the items of the example inventory.'
    FunctionsToExport = @(
        'Get-ExampleItem'
        'Set-ExampleItem'
        'Remove-ExampleItem'
    )
    CmdletsToExport   = @()
    AliasesToExport   = @('gei')
    # Worked out when the module loads: never read.
    ScriptsToProcess  = @("$PSScriptRoot\Example.Setup.ps1")
    PrivateData       = @{
        PSData = @{
            Tags = @('example', 'inventory')
        }
    }
}
