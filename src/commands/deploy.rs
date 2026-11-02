use crate::cli::DeployArgs;
use crate::core::project::Project;

pub fn run(p: &Project, args: DeployArgs) -> anyhow::Result<()> {
    crate::deploy::run(p, args.edit, args.dry_run)
}
