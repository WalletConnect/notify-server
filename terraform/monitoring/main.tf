terraform {
  required_version = "~> 1.0"

  required_providers {
    grafana = {
      source  = "grafana/grafana"
      version = "~> 1.24"
    }
  }
}

locals {
  load_balancer = join("/", slice(split("/", var.load_balancer_arn), 1, 4))

  opsgenie_notification_channel = "l_iaPw6nk"
  notifications = (
    var.environment == "prod" ?
    "[{\"uid\": \"${local.opsgenie_notification_channel}\"}]" :
    "[]"
  )
}

resource "grafana_data_source" "prometheus" {
  type = "prometheus"
  name = "${var.environment}-${var.app_name}-amp"
  url  = "https://aps-workspaces.eu-central-1.amazonaws.com/workspaces/${var.prometheus_workspace_id}/"

  json_data {
    http_method     = "GET"
    sigv4_auth      = true
    sigv4_auth_type = "workspace-iam-role"
    sigv4_region    = "eu-central-1"
  }
}

resource "grafana_data_source" "cloudwatch" {
  type = "cloudwatch"
  name = "${var.environment}-${var.app_name}-cloudwatch"

  json_data {
    default_region = "eu-central-1"
  }
}

# JSON Dashboard. When exporting from Grafana make sure that all
# variables are replaced properly
resource "grafana_dashboard" "at_a_glance" {
  overwrite = true
  message   = "Updated by Terraform"
  config_json = jsonencode(
    {
      "annotations" : {
        "list" : [
          {
            "builtIn" : 1,
            "datasource" : "-- Grafana --",
            "enable" : true,
            "hide" : true,
            "iconColor" : "rgba(0, 211, 255, 1)",
            "name" : "Annotations & Alerts",
            "target" : {
              "limit" : 100,
              "matchAny" : false,
              "tags" : [],
              "type" : "dashboard"
            },
            "type" : "dashboard"
          }
        ]
      },
      "editable" : true,
      "fiscalYearStartMonth" : 0,
      "graphTooltip" : 0,
      "id" : 52,
      "links" : [],
      "liveNow" : false,
      "panels" : [
        {
          "collapsed" : false,
          "gridPos" : {
            "h" : 1,
            "w" : 24,
            "x" : 0,
            "y" : 0
          },
          "id" : 13,
          "panels" : [],
          "title" : "Database",
          "type" : "row"
        },
        {
          "datasource" : {
            "type" : "cloudwatch",
            "uid" : grafana_data_source.cloudwatch.uid
          },
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "mode" : "palette-classic"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "area"
                }
              },
              "mappings" : [],
              "max" : 16000000000,
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "#EAB839",
                    "value" : 6000000000
                  },
                  {
                    "color" : "red",
                    "value" : 12000000000
                  }
                ]
              },
              "unit" : "decbytes"
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 8,
            "w" : 12,
            "x" : 0,
            "y" : 1
          },
          "id" : 19,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "alias" : "",
              "datasource" : {
                "type" : "cloudwatch",
                "uid" : grafana_data_source.cloudwatch.uid
              },
              "dimensions" : {
                "DBClusterIdentifier" : var.document_db_cluster_id
              },
              "expression" : "",
              "id" : "",
              "matchExact" : true,
              "metricEditorMode" : 0,
              "metricName" : "FreeableMemory",
              "metricQueryType" : 0,
              "namespace" : "AWS/DocDB",
              "period" : "",
              "queryMode" : "Metrics",
              "refId" : "A",
              "region" : "default",
              "sqlExpression" : "",
              "statistic" : "Average"
            }
          ],
          "title" : "Memory Usage",
          "type" : "timeseries"
        },
        {
          "datasource" : {
            "type" : "cloudwatch",
            "uid" : grafana_data_source.cloudwatch.uid
          },
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "mode" : "continuous-GrYlRd"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "area"
                }
              },
              "mappings" : [],
              "max" : 90,
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "#EAB839",
                    "value" : 40
                  },
                  {
                    "color" : "red",
                    "value" : 80
                  }
                ]
              },
              "unit" : "percent"
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 8,
            "w" : 12,
            "x" : 12,
            "y" : 1
          },
          "id" : 15,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "alias" : "",
              "datasource" : {
                "type" : "cloudwatch",
                "uid" : grafana_data_source.cloudwatch.uid
              },
              "dimensions" : {
                "DBClusterIdentifier" : var.document_db_cluster_id
              },
              "expression" : "",
              "id" : "",
              "matchExact" : true,
              "metricEditorMode" : 0,
              "metricName" : "CPUUtilization",
              "metricQueryType" : 0,
              "namespace" : "AWS/DocDB",
              "period" : "",
              "queryMode" : "Metrics",
              "refId" : "A",
              "region" : "default",
              "sqlExpression" : "",
              "statistic" : "Average"
            }
          ],
          "title" : "CPU Utilization",
          "type" : "timeseries"
        },
        {
          "datasource" : {
            "type" : "cloudwatch",
            "uid" : grafana_data_source.cloudwatch.uid
          },
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "mode" : "palette-classic"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "off"
                }
              },
              "mappings" : [],
              "max" : 1000000,
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "red",
                    "value" : 80
                  }
                ]
              },
              "unit" : "decbytes"
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 8,
            "w" : 12,
            "x" : 0,
            "y" : 9
          },
          "id" : 17,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "alias" : "",
              "datasource" : {
                "type" : "cloudwatch",
                "uid" : grafana_data_source.cloudwatch.uid
              },
              "dimensions" : {
                "DBClusterIdentifier" : var.document_db_cluster_id
              },
              "expression" : "",
              "id" : "",
              "matchExact" : true,
              "metricEditorMode" : 0,
              "metricName" : "NetworkThroughput",
              "metricQueryType" : 0,
              "namespace" : "AWS/DocDB",
              "period" : "",
              "queryMode" : "Metrics",
              "refId" : "A",
              "region" : "default",
              "sqlExpression" : "",
              "statistic" : "Average"
            }
          ],
          "title" : "Network Throughput",
          "type" : "timeseries"
        },
        {
          "datasource" : {
            "type" : "cloudwatch",
            "uid" : grafana_data_source.cloudwatch.uid
          },
          "description" : "",
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "mode" : "palette-classic"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "area"
                }
              },
              "mappings" : [],
              "max" : 0.1,
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "#EAB839",
                    "value" : 0.015
                  },
                  {
                    "color" : "red",
                    "value" : 0.05
                  }
                ]
              },
              "unit" : "s"
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 8,
            "w" : 12,
            "x" : 12,
            "y" : 9
          },
          "id" : 21,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "alias" : "",
              "datasource" : {
                "type" : "cloudwatch",
                "uid" : grafana_data_source.cloudwatch.uid
              },
              "dimensions" : {
                "DBClusterIdentifier" : var.document_db_cluster_id
              },
              "expression" : "",
              "id" : "",
              "matchExact" : true,
              "metricEditorMode" : 0,
              "metricName" : "WriteLatency",
              "metricQueryType" : 0,
              "namespace" : "AWS/DocDB",
              "period" : "",
              "queryMode" : "Metrics",
              "refId" : "A",
              "region" : "default",
              "sqlExpression" : "",
              "statistic" : "Average"
            }
          ],
          "title" : "Write Latency",
          "type" : "timeseries"
        },
        {
          "collapsed" : true,
          "gridPos" : {
            "h" : 1,
            "w" : 24,
            "x" : 0,
            "y" : 17
          },
          "id" : 11,
          "panels" : [],
          "title" : "Graphs",
          "type" : "row"
        },
        {
          "datasource" : {
            "type" : "prometheus",
            "uid" : grafana_data_source.prometheus.uid
          },
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "mode" : "palette-classic"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "off"
                }
              },
              "mappings" : [],
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "red",
                    "value" : 80
                  }
                ]
              }
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 8,
            "w" : 12,
            "x" : 0,
            "y" : 18
          },
          "id" : 2,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "datasource" : {
                "type" : "prometheus",
                "uid" : grafana_data_source.prometheus.uid
              },
              "exemplar" : true,
              "expr" : "sum(increase(registered_clients[5m]))",
              "interval" : "",
              "legendFormat" : "",
              "refId" : "A"
            }
          ],
          "title" : "Client Registrations",
          "type" : "timeseries"
        },
        {
          "datasource" : {
            "type" : "prometheus",
            "uid" : grafana_data_source.prometheus.uid
          },
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "mode" : "palette-classic"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineStyle" : {
                  "fill" : "solid"
                },
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "off"
                }
              },
              "mappings" : [],
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "red",
                    "value" : 80
                  }
                ]
              }
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 8,
            "w" : 12,
            "x" : 12,
            "y" : 18
          },
          "id" : 23,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "datasource" : {
                "type" : "prometheus",
                "uid" : grafana_data_source.prometheus.uid
              },
              "exemplar" : true,
              "expr" : "increase(dispatched_notifications{type=\"sent\"}[5m])",
              "interval" : "",
              "legendFormat" : "Sent push",
              "refId" : "A"
            }
          ],
          "title" : "Dispatched Notifications",
          "type" : "timeseries"
        },
        {
          "datasource" : {
            "type" : "prometheus",
            "uid" : grafana_data_source.prometheus.uid
          },
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "fixedColor" : "semi-dark-red",
                "mode" : "fixed"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineStyle" : {
                  "fill" : "solid"
                },
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "off"
                }
              },
              "mappings" : [],
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "red",
                    "value" : 80
                  }
                ]
              }
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 8,
            "w" : 12,
            "x" : 0,
            "y" : 26
          },
          "id" : 24,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "datasource" : {
                "type" : "prometheus",
                "uid" : grafana_data_source.prometheus.uid
              },
              "exemplar" : true,
              "expr" : "increase(dispatched_notifications{type=\"failed\"}[5m])",
              "interval" : "",
              "legendFormat" : "Failed",
              "refId" : "A"
            }
          ],
          "title" : "Failed to send",
          "type" : "timeseries"
        },
        {
          "datasource" : {
            "type" : "prometheus",
            "uid" : grafana_data_source.prometheus.uid
          },
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "fixedColor" : "light-yellow",
                "mode" : "fixed"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineStyle" : {
                  "fill" : "solid"
                },
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "off"
                }
              },
              "mappings" : [],
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "red",
                    "value" : 80
                  }
                ]
              }
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 8,
            "w" : 12,
            "x" : 12,
            "y" : 26
          },
          "id" : 25,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "datasource" : {
                "type" : "prometheus",
                "uid" : grafana_data_source.prometheus.uid
              },
              "exemplar" : true,
              "expr" : "increase(dispatched_notifications{type=\"not_found\"}[5m])",
              "interval" : "",
              "legendFormat" : "Not found",
              "refId" : "A"
            }
          ],
          "title" : "Not found accounts",
          "type" : "timeseries"
        },
        {
          "collapsed" : false,
          "gridPos" : {
            "h" : 1,
            "w" : 24,
            "x" : 0,
            "y" : 34
          },
          "id" : 9,
          "panels" : [],
          "title" : "AWS Load Balancer",
          "type" : "row"
        },
        {
          "datasource" : {
            "type" : "cloudwatch",
            "uid" : grafana_data_source.cloudwatch.uid
          },
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "mode" : "palette-classic"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "off"
                }
              },
              "mappings" : [],
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "red",
                    "value" : 80
                  }
                ]
              }
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 10,
            "w" : 7,
            "x" : 0,
            "y" : 35
          },
          "id" : 4,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "alias" : "",
              "datasource" : {
                "type" : "cloudwatch",
                "uid" : grafana_data_source.cloudwatch.uid
              },
              "dimensions" : {
                "LoadBalancer" : local.load_balancer
              },
              "expression" : "",
              "id" : "",
              "matchExact" : true,
              "metricEditorMode" : 0,
              "metricName" : "HTTPCode_ELB_4XX_Count",
              "metricQueryType" : 0,
              "namespace" : "AWS/ApplicationELB",
              "period" : "",
              "queryMode" : "Metrics",
              "refId" : "A",
              "region" : "default",
              "sqlExpression" : "",
              "statistic" : "Sum"
            },
            {
              "alias" : "",
              "datasource" : {
                "type" : "cloudwatch",
                "uid" : grafana_data_source.cloudwatch.uid
              },
              "dimensions" : {
                "LoadBalancer" : local.load_balancer
              },
              "expression" : "",
              "hide" : false,
              "id" : "",
              "matchExact" : true,
              "metricEditorMode" : 0,
              "metricName" : "HTTPCode_Target_4XX_Count",
              "metricQueryType" : 0,
              "namespace" : "AWS/ApplicationELB",
              "period" : "",
              "queryMode" : "Metrics",
              "refId" : "B",
              "region" : "default",
              "sqlExpression" : "",
              "statistic" : "Sum"
            }
          ],
          "title" : "4XX",
          "type" : "timeseries"
        },
        {
          "datasource" : {
            "type" : "cloudwatch",
            "uid" : grafana_data_source.cloudwatch.uid
          },
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "mode" : "palette-classic"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "off"
                }
              },
              "mappings" : [],
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "red",
                    "value" : 80
                  }
                ]
              }
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 10,
            "w" : 7,
            "x" : 7,
            "y" : 35
          },
          "id" : 5,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "alias" : "",
              "datasource" : {
                "type" : "cloudwatch",
                "uid" : grafana_data_source.cloudwatch.uid
              },
              "dimensions" : {
                "LoadBalancer" : local.load_balancer
              },
              "expression" : "",
              "id" : "",
              "matchExact" : true,
              "metricEditorMode" : 0,
              "metricName" : "HTTPCode_ELB_5XX_Count",
              "metricQueryType" : 0,
              "namespace" : "AWS/ApplicationELB",
              "period" : "",
              "queryMode" : "Metrics",
              "refId" : "A",
              "region" : "default",
              "sqlExpression" : "",
              "statistic" : "Sum"
            },
            {
              "alias" : "",
              "datasource" : {
                "type" : "cloudwatch",
                "uid" : grafana_data_source.cloudwatch.uid
              },
              "dimensions" : {
                "LoadBalancer" : local.load_balancer
              },
              "expression" : "",
              "hide" : false,
              "id" : "",
              "matchExact" : true,
              "metricEditorMode" : 0,
              "metricName" : "HTTPCode_Target_5XX_Count",
              "metricQueryType" : 0,
              "namespace" : "AWS/ApplicationELB",
              "period" : "",
              "queryMode" : "Metrics",
              "refId" : "B",
              "region" : "default",
              "sqlExpression" : "",
              "statistic" : "Sum"
            }
          ],
          "title" : "5XX",
          "type" : "timeseries"
        },
        {
          "datasource" : {
            "type" : "cloudwatch",
            "uid" : grafana_data_source.cloudwatch.uid
          },
          "fieldConfig" : {
            "defaults" : {
              "color" : {
                "mode" : "palette-classic"
              },
              "custom" : {
                "axisLabel" : "",
                "axisPlacement" : "auto",
                "barAlignment" : 0,
                "drawStyle" : "line",
                "fillOpacity" : 0,
                "gradientMode" : "none",
                "hideFrom" : {
                  "legend" : false,
                  "tooltip" : false,
                  "viz" : false
                },
                "lineInterpolation" : "linear",
                "lineWidth" : 1,
                "pointSize" : 5,
                "scaleDistribution" : {
                  "type" : "linear"
                },
                "showPoints" : "auto",
                "spanNulls" : false,
                "stacking" : {
                  "group" : "A",
                  "mode" : "none"
                },
                "thresholdsStyle" : {
                  "mode" : "off"
                }
              },
              "mappings" : [],
              "thresholds" : {
                "mode" : "absolute",
                "steps" : [
                  {
                    "color" : "green",
                    "value" : null
                  },
                  {
                    "color" : "red",
                    "value" : 80
                  }
                ]
              }
            },
            "overrides" : []
          },
          "gridPos" : {
            "h" : 10,
            "w" : 7,
            "x" : 14,
            "y" : 35
          },
          "id" : 7,
          "options" : {
            "legend" : {
              "calcs" : [],
              "displayMode" : "list",
              "placement" : "bottom"
            },
            "tooltip" : {
              "mode" : "single",
              "sort" : "none"
            }
          },
          "targets" : [
            {
              "alias" : "",
              "datasource" : {
                "type" : "cloudwatch",
                "uid" : grafana_data_source.cloudwatch.uid
              },
              "dimensions" : {
                "LoadBalancer" : local.load_balancer
              },
              "expression" : "",
              "id" : "",
              "matchExact" : true,
              "metricEditorMode" : 0,
              "metricName" : "RequestCount",
              "metricQueryType" : 0,
              "namespace" : "AWS/ApplicationELB",
              "period" : "",
              "queryMode" : "Metrics",
              "refId" : "A",
              "region" : "default",
              "sqlExpression" : "",
              "statistic" : "Sum"
            }
          ],
          "title" : "Request",
          "type" : "timeseries"
        }
      ],
      "refresh" : false,
      "schemaVersion" : 35,
      "style" : "dark",
      "tags" : [],
      "templating" : {
        "list" : []
      },
      "time" : {
        "from" : "now-6h",
        "to" : "now"
      },
      "timepicker" : {},
      "timezone" : "",
      "title" : var.app_name,
      "uid" : var.app_name,
      "version" : 9,
      "weekStart" : ""
    }
  )
}
