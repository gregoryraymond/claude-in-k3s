variable "k3s_binary_path" {
  description = "Path to the k3s binary"
  type        = string
  default     = "/usr/local/bin/k3s"
}

variable "kubeconfig_path" {
  description = "Path to the k3s kubeconfig file"
  type        = string
  default     = "/etc/rancher/k3s/k3s.yaml"
}

variable "kubeconfig_output_path" {
  description = "Where to copy the kubeconfig for non-root access"
  type        = string
  default     = "~/.kube/config"
}

variable "manage_service" {
  description = "Whether Terraform should manage the k3s systemd service lifecycle"
  type        = bool
  default     = true
}

variable "platform" {
  description = "Operating system: linux, macos, wsl2, or windows"
  type        = string
  default     = "linux"
}
