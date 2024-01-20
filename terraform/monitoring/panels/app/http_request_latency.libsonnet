local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'HTTP Req Latency',
      datasource  = ds.prometheus,
    )
    .configure(
      defaults.configuration.timeseries
        .withUnit('ms')
    )

    .setAlert(vars.environment, grafana.alert.new(
      namespace     = vars.namespace,
      name          = '%(env)s - HTTP request latency too high' % { env: vars.environment },
      message       = '%(env)s - HTTP request latency too high' % { env: vars.environment },
      notifications = vars.notifications,
      noDataState   = 'no_data',
      conditions    = [
        grafana.alertCondition.new(
          evaluatorParams = [ 3000 ],
          evaluatorType   = 'gt',
          operatorType    = 'or',
          queryRefId      = 'HttpRequestLatency',
          queryTimeStart  = '5m',
          queryTimeEnd    = 'now',
          reducerType     = grafana.alert_reducers.Avg
        ),
      ],
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision, method, endpoint) (rate(http_request_latency_sum[$__rate_interval])) / sum by (aws_ecs_task_revision, method, endpoint) (rate(http_request_latency_count[$__rate_interval]))',
      legendFormat  = '{{method}} {{endpoint}} r{{aws_ecs_task_revision}}',
      exemplar      = false,
      refId         = 'HttpRequestLatency',
    ))
}
