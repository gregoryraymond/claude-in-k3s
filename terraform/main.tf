# Root module - orchestrates K3s cluster lifecycle
# Expects k3s to already be installed on the host

locals {
  namespace = "claude-code"
}

# Create the namespace for Claude Code pods
resource "terraform_data" "claude_namespace" {
  depends_on = [terraform_data.cluster_health_check]

  input = local.namespace

  provisioner "local-exec" {
    command = "kubectl create namespace ${local.namespace} --dry-run=client -o yaml | kubectl apply -f -"
  }

  provisioner "local-exec" {
    when    = destroy
    command = "kubectl delete namespace ${self.input} --ignore-not-found=true"
  }
}
