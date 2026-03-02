output "kubeconfig_path" {
  description = "Path to the kubeconfig file"
  value       = var.kubeconfig_output_path
}

output "k3s_binary_path" {
  description = "Path to the k3s binary"
  value       = var.k3s_binary_path
}

output "namespace" {
  description = "Kubernetes namespace for Claude Code pods"
  value       = local.namespace
}
