function [T, meta, R] = lightsim_run(project, caseName, varargin)
%LIGHTSIM_RUN Run a LightSim case and return its results as a table.
%
%   T = lightsim_run('car.json', 'WLTC Class 3b') runs the case named
%   'WLTC Class 3b' (or with that id) of the LightSim project file car.json
%   with LightSim's engine and returns a table: column t (s) and one column
%   per result channel, named Part_Channel (E_Motor_Shaft_Torque). Units
%   are in T.Properties.VariableUnits, the names as LightSim shows them in
%   T.Properties.VariableDescriptions.
%
%   [T, meta] = lightsim_run(...) also returns the run details: project,
%   case, app version, model fingerprint, status, the summary values,
%   the parameters that differ from the library defaults, and the figures
%   the run's checks rule out (meta.not_valid).
%
%   [T, meta, R] = lightsim_run(...) also returns the .mat file as loaded:
%   one struct per part (R.E_Motor.t, R.E_Motor.Shaft_Torque).
%
%   Options, as name-value pairs:
%     'Engine'  the engine to run: the path of lightsim-backend (.exe), or
%               a command such as 'python /path/to/backend/run_backend.py'.
%               Default: the LIGHTSIM_ENGINE environment variable, else
%               the engine next to this file (LightSim installs it in its
%               resources\matlab folder, beside resources\backend), else
%               the engine of an installed LightSim.
%     'Out'     where to keep the .mat file (default: a temporary file,
%               deleted afterwards).
%
%   The run uses only your computer: no network, no window opens. A case
%   that cannot run (Data Checks found errors) raises an error with the
%   reasons; a run with figures that are not valid gives a warning.
%
%   Example:
%     T = lightsim_run('FS Electric.json', 'Acceleration 75 m');
%     plot(T.t, T.Vehicle_Vehicle_Speed); xlabel('t [s]'); ylabel('km/h')
%
%   Part of LightSim. See "Use LightSim results in MATLAB and Python" in
%   LightSim's help.

    opts = struct('Engine', '', 'Out', '');
    for k = 1:2:numel(varargin)
        name = varargin{k};
        if ~isfield(opts, name)
            error('lightsim:option', 'Unknown option ''%s''. Options: Engine, Out.', name);
        end
        opts.(name) = varargin{k + 1};
    end
    if nargin < 2 || isempty(caseName)
        caseName = '';
    end
    if exist(project, 'file') ~= 2
        error('lightsim:project', 'No project file ''%s''.', project);
    end

    engine = find_engine(opts.Engine);
    out = opts.Out;
    keep = ~isempty(out);
    if ~keep
        out = [tempname() '.mat'];
    end

    cmd = sprintf('%s run %s --out %s --json', engine, quote(project), quote(out));
    if ~isempty(caseName)
        cmd = sprintf('%s --case %s', cmd, quote(caseName));
    end
    cmd = [cmd ' 2>&1'];  % its messages too, not only its output
    if ispc
        cmd = ['"' cmd '"'];  % cmd.exe keeps the inner quotes
    end
    [status, text] = system(cmd);

    if status == 1
        error('lightsim:checks', 'LightSim did not run the case:\n%s', strtrim(text));
    elseif status == 3 || ~exist(out, 'file')
        error('lightsim:engine', 'LightSim could not run (%d):\n%s', status, strtrim(text));
    end

    R = load(out);
    if ~keep
        delete(out);
    end
    meta = R.meta;
    if status == 2
        reasons = meta.not_valid;
        if iscell(reasons) && ~isempty(reasons)
            warning('lightsim:notValid', 'Some results are not valid:\n  %s', ...
                strjoin(reasons(:)', '\n  '));
        else
            warning('lightsim:notValid', 'The run ended with status ''%s''.', meta.status);
        end
    end
    T = as_table(R);
end


function T = as_table(R)
    % one column per channel, all on the first part's time
    parts = setdiff(fieldnames(R), {'meta'}, 'stable');
    names = {'t'};
    units = {'s'};
    labels = {'Time'};
    cols = {};
    t = [];
    for i = 1:numel(parts)
        p = parts{i};
        s = R.(p);
        if isempty(t)
            t = s.t(:);
        end
        f = setdiff(fieldnames(s), {'t'}, 'stable');
        for j = 1:numel(f)
            v = s.(f{j});
            if numel(v) ~= numel(t)
                continue  % a channel on a time base of its own stays in R
            end
            names{end + 1} = [p '_' f{j}]; %#ok<AGROW>
            units{end + 1} = R.meta.units.(p).(f{j}); %#ok<AGROW>
            labels{end + 1} = R.meta.labels.(p).(f{j}); %#ok<AGROW>
            cols{end + 1} = v(:); %#ok<AGROW>
        end
    end
    names = valid_names(names);
    if exist('table', 'class') == 8 || exist('table', 'file') == 2
        T = table(t, cols{:}, 'VariableNames', names);
        T.Properties.VariableUnits = units;
        T.Properties.VariableDescriptions = labels;
    else
        % GNU Octave has no table: a struct of columns, units in T.units
        T = struct('t', t);
        for j = 2:numel(names)
            T.(names{j}) = cols{j - 1};
        end
        T.units = cell2struct(units(:), names(:), 1);
    end
end


function names = valid_names(names)
    if exist('matlab.lang.makeValidName', 'file')
        names = matlab.lang.makeUniqueStrings(matlab.lang.makeValidName(names), {}, namelengthmax);
    else
        names = cellfun(@(n) n(1:min(end, 63)), names, 'UniformOutput', false);
    end
end


function q = quote(s)
    q = ['"' strrep(s, '"', '\"') '"'];
end


function engine = find_engine(given)
    if ~isempty(given)
        engine = given;
        if exist(engine, 'file') == 2
            engine = quote(engine);
        end
        return
    end
    env = getenv('LIGHTSIM_ENGINE');
    if ~isempty(env)
        engine = find_engine(env);
        return
    end
    % the copy installed with LightSim sits in resources/matlab, next to
    % resources/backend: that engine first, so the script and the engine
    % come from the same LightSim
    here = fileparts(mfilename('fullpath'));
    if ispc
        candidates = {
            fullfile(here, '..', 'backend', 'lightsim-backend.exe')
            fullfile(getenv('LOCALAPPDATA'), 'Programs', 'LightSim', 'resources', 'backend', 'lightsim-backend.exe')
            fullfile(getenv('ProgramFiles'), 'LightSim', 'resources', 'backend', 'lightsim-backend.exe')
        };
    else
        candidates = {
            fullfile(here, '..', 'backend', 'lightsim-backend')
            '/opt/LightSim/resources/backend/lightsim-backend'
            '/Applications/LightSim.app/Contents/Resources/backend/lightsim-backend'
            fullfile(getenv('HOME'), 'LightSim', 'resources', 'backend', 'lightsim-backend')
        };
    end
    for i = 1:numel(candidates)
        if exist(candidates{i}, 'file') == 2
            engine = quote(candidates{i});
            return
        end
    end
    error('lightsim:engine', ['LightSim''s engine was not found. Install LightSim, or pass ' ...
        'its path: lightsim_run(project, case, ''Engine'', ''C:\\...\\resources\\backend\\' ...
        'lightsim-backend.exe''), or set the LIGHTSIM_ENGINE environment variable.']);
end
