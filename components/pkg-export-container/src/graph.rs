use crate::build::BasePkgIdents;
use anyhow::Result;
use habitat_common::package_graph::PackageGraph;
use habitat_core::package::{FullyQualifiedPackageIdent,
                            PackageIdent};
use linked_hash_map::LinkedHashMap;
use std::path::Path;

pub(crate) struct Graph {
    g:    PackageGraph,
    base: BasePkgIdents,
    user: Vec<FullyQualifiedPackageIdent>,
}

impl Graph {
    pub(crate) fn from_packages(base: BasePkgIdents,
                                user: Vec<FullyQualifiedPackageIdent>,
                                rootfs: &Path)
                                -> Result<Graph> {
        let g = PackageGraph::from_root_path(rootfs)?;
        Ok(Graph { g, base, user })
    }

    /// Helper function to create a Vec of our base idents in a
    /// sensible order (roughly in order of package volatility).
    ///
    /// The idea is that we'll add packages to the image in this
    /// order, one layer at a time, in order to try and maximize
    /// layer caching.
    fn idents_from_base(&self) -> Vec<PackageIdent> {
        // TODO (CM): Yes, we store the idents natively as
        // fully-qualified, but the type abstraction for that isn't
        // fully done yet, and the underlying PackageGraph hasn't yet
        // been converted to transparently handle FQPIs. Thus, we use
        // this as a boundary point and use `as_ref()` calls to get at
        // the underlying PackageIdents directly.

        let mut idents = vec![];
        if let Some(ref busybox) = self.base.busybox {
            idents.push(busybox.as_ref().clone());
        }
        idents.push(self.base.launcher.as_ref().clone());
        idents.push(self.base.hab.as_ref().clone());
        idents.push(self.base.sup.as_ref().clone());
        idents.push(self.base.cacerts.as_ref().clone());
        idents
    }

    /// Similarly to how `idents_from_base` returns a
    /// `Vec<PackageIdent>` to form a boundary between this type and
    /// the underlying package graph, we do the same thing here. Once
    /// the type abstractions are more harmonized, we can dispense
    /// with this.
    fn user_idents(&self) -> Vec<PackageIdent> {
        self.user.iter().map(|fqpi| fqpi.as_ref().clone()).collect()
    }

    /// Return the list of packages to install in the image in
    /// dependency order.
    ///
    /// Note that this is essentially a consistent union of the
    /// reverse topological sorts of all the "top level" packages that
    /// are added to a container image.
    ///
    /// User packages will be last. Ideally, as users are iterating on
    /// their packages and creating images, this should mean that all
    /// the dependencies are already available as cached layers.
    pub(crate) fn reverse_topological_sort(&self) -> Vec<PackageIdent> {
        self.idents_from_base()
            .into_iter()
            .chain(self.user_idents())
            .flat_map(|ident| {
                let mut pkgs = self.g.owned_ordered_deps(&ident);
                // We want the most basic dependencies first.
                pkgs.reverse();
                // owned_ordered_deps does not include the given
                // ident, so let's add it.
                pkgs.push(ident);
                pkgs
            })
            .fold(LinkedHashMap::new(), |mut acc, ident| {
                // NOTE: We are using LinkedHashMap here to simulate
                // an insertion-order-preserving Set. As of this
                // writing (April 2020), however, LinkedHashMap is in
                // maintenance mode. It is still used by things we
                // depend on, though, so we're already using it,
                // regardless. If this becomes problematic in the future,
                // we can always revert to using a Vec directly. It's
                // not as efficient, of course, but this call is not
                // likely to be any sort of bottlneck in the creation
                // of a container image.

                // You have to check first before inserting;
                // otherwise, it increments the insertion order
                // each time, which will give us an incorrect
                // overall ordering.
                if !acc.contains_key(&ident) {
                    // Treat this map like a set
                    acc.insert(ident, ());
                }
                acc
            })
            .into_iter()
            .map(|(k, _v)| k)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryFrom;

    /// Helper macro to create PackageIdents and make them easily
    /// accessible via function invocation.
    macro_rules! pkg {
        ($fn_name:ident, $ident_str:expr) => {
            fn $fn_name() -> PackageIdent { $ident_str.parse().unwrap() }
        };
    }

    /// Helper macro to convert a PackageIdent into a
    /// FullyQualifiedPackageIdent with a minimum of ceremony.
    macro_rules! fqpi {
        ($ident:expr) => {
            FullyQualifiedPackageIdent::try_from($ident).unwrap()
        };
    }

    // These are all the packages needed for hab, hab-sup,
    // hab-launcher and redis

    // These are the packages that go into every container (well,
    // specific *releases* of those packages, for the purpose of
    // testing).
    //
    // Yes, these are properly all FullyQualifiedPackageIdents, but
    // the PackageGraph doesn't yet handle those.
    pkg!(hab, "chef/hab/1.5.71/20200318171932");
    pkg!(sup, "chef/hab-sup/1.5.71/20200318174937");
    pkg!(launcher, "chef/hab-launcher/13458/20200318174911");
    pkg!(busybox, "core/busybox-static/1.29.2/20190115014552");
    pkg!(cacerts, "core/cacerts/2018.12.05/20190115014206");

    // This would be an example of a service a container is being
    // exported for.
    pkg!(redis, "core/redis/4.0.14/20190319155852");

    // These are all the dependencies of all of the above
    pkg!(gcc_libs, "core/gcc-libs/8.2.0/20190115011926");
    pkg!(glibc, "core/glibc/2.27/20190115002733");
    pkg!(libsodium, "core/libsodium/1.0.16/20190116014025");
    pkg!(linux_headers, "core/linux-headers/4.17.12/20190115002705");
    pkg!(zeromq, "core/zeromq/4.3.1/20190802173651");

    /// Create a Graph manually, bypassing the need to generate one
    /// based on the package contents of a local directory.
    fn test_graph() -> Graph {
        let mut graph = PackageGraph::default();

        // hab, busybox, and cacerts have no dependencies
        graph.extend(&hab(), &[]);
        graph.extend(&busybox(), &[]);
        graph.extend(&cacerts(), &[]);

        // Launcher and its dependencies
        graph.extend(&launcher(), &[gcc_libs(), glibc()]);
        graph.extend(&gcc_libs(), &[glibc()]);
        graph.extend(&glibc(), &[linux_headers()]);

        // Supervisor and its dependencies
        graph.extend(&sup(), &[busybox(), gcc_libs(), glibc(), zeromq()]);
        graph.extend(&zeromq(), &[gcc_libs(), glibc(), libsodium()]);

        // User package and its dependencies
        graph.extend(&redis(), &[glibc()]);

        let base = BasePkgIdents { hab:      fqpi!(hab()),
                                   sup:      fqpi!(sup()),
                                   launcher: fqpi!(launcher()),
                                   busybox:  Some(fqpi!(busybox())),
                                   cacerts:  fqpi!(cacerts()), };

        let user = vec![fqpi!(redis())];

        Graph { base,
                user,
                g: graph }
    }

    #[test]
    fn reverse_topological_sort_produces_the_correct_ordering() {
        let g = test_graph();

        let actual_deps = g.reverse_topological_sort();
        let expected_deps = [// busybox
                             busybox(),
                             // launcher
                             linux_headers(),
                             gcc_libs(),
                             glibc(),
                             launcher(),
                             // hab
                             hab(),
                             // sup
                             libsodium(),
                             zeromq(),
                             sup(),
                             // user package(s)
                             cacerts(),
                             redis()];

        assert_eq!(actual_deps, expected_deps);
    }
}
