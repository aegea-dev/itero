# terraform

A k3d cluster of 3 machines (containers in Docker) with a web app on it,
applied with OpenTofu, all on this machine. Run `itero daemon` in this
folder, then from another terminal:

  itero emit --wait tools.requested
      installs k3d, kubectl and OpenTofu with Homebrew (the Brewfile), only
      what is missing; it changes the machine, so it runs only when asked
  itero emit --wait --timeout 10m cluster.requested '{"name": "demo"}'
      the cluster, then the app planned, checked, applied and asked:
      whoami at http://localhost:8080 ({"port": 8081} for another)
  itero emit --wait cluster.removed '{"name": "demo"}'

A step that fails rolls back: #cluster-create's undo deletes the cluster,
and a notification says what broke (and what is left, when the delete
breaks too). Your own ~/.kube/config is left alone; each cluster's
kubeconfig, OpenTofu state and plan stay in .itero/clusters/<name>/.
